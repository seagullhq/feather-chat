package accounts

import (
	"context"
	"crypto/rand"
	"database/sql"
	"errors"

	"golang.org/x/crypto/bcrypt"

	"github.com/seagullhq/feather-chat/server/internal/e2ee"
)

var (
	ErrNoKeys    = errors.New("user keys not found")
	ErrKeysExist = errors.New("account keys already set")
	ErrBadKeys   = errors.New("invalid account keys")
)

type queryer interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

func currentKeys(ctx context.Context, q queryer, userID int) (*e2ee.AccountKeys, []byte, error) {
	var k e2ee.AccountKeys
	var recovery []byte
	err := q.QueryRowContext(ctx, `
		SELECT id, identity_pub, enc_pub, key_sig, ak_password, ak_recovery, bundle, recovery_verifier
		FROM user_keys WHERE user_id = $1 AND retired_at IS NULL`, userID).
		Scan(&k.KeyID, &k.IdentityPub, &k.EncPub, &k.KeySig, &k.AKPassword, &k.AKRecovery, &k.Bundle, &recovery)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil, ErrNoKeys
	}
	if err != nil {
		return nil, nil, err
	}
	return &k, recovery, nil
}

// CurrentKeys returns the account's keys, or ErrNoKeys.
func (s *Store) CurrentKeys(ctx context.Context, userID int) (*e2ee.AccountKeys, error) {
	k, _, err := currentKeys(ctx, s.db, userID)
	return k, err
}

// lockCredential locks the user row and returns what is needed to compare
// credentials. Every change of credentials or keys takes this lock first.
func lockCredential(ctx context.Context, tx *sql.Tx, userID int) (*User, error) {
	u := User{ID: userID}
	err := tx.QueryRowContext(ctx, `
		SELECT password_hash, auth_scheme, auth_verifier FROM users WHERE id = $1 FOR UPDATE`, userID).
		Scan(&u.PasswordHash, &u.AuthScheme, &u.Verifier)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	return &u, err
}

func setAuthKey(ctx context.Context, tx *sql.Tx, userID int, salt, authKey []byte) error {
	_, err := tx.ExecContext(ctx, `
		UPDATE users SET auth_scheme = 1, password_hash = '', auth_salt = $2, auth_verifier = $3, updated_at = now()
		WHERE id = $1`, userID, salt, verifier(authKey))
	return err
}

// SetKeys gives an account without keys its first ones, replacing a bcrypt hash
// by salt and authKey. credential is what the session logged in against. It
// returns the stored keys and the credential now in force.
func (s *Store) SetKeys(ctx context.Context, userID int, credential string, k *e2ee.AccountKeys, recoveryKey, salt, authKey []byte) (*e2ee.AccountKeys, string, error) {
	if k == nil || k.KeyID != 0 || len(k.IdentityPub) != e2ee.KeyLen || len(k.EncPub) != e2ee.KeyLen ||
		len(k.AKPassword) != e2ee.WrappedAKLen || len(k.AKRecovery) != e2ee.WrappedAKLen ||
		len(k.Bundle) != e2ee.BundleLen || len(recoveryKey) != e2ee.KeyLen ||
		!e2ee.Verify(k.IdentityPub, e2ee.KeyRecord(userID, k.IdentityPub, k.EncPub), k.KeySig) {
		return nil, "", ErrBadKeys
	}
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return nil, "", err
	}
	defer tx.Rollback()

	user, err := lockCredential(ctx, tx, userID)
	if err != nil {
		return nil, "", err
	}
	if _, _, err := currentKeys(ctx, tx, userID); err == nil {
		return nil, "", ErrKeysExist
	} else if !errors.Is(err, ErrNoKeys) {
		return nil, "", err
	}
	if user.Credential() != credential {
		return nil, "", ErrBadCredentials // reset or changed since LOGIN
	}
	if user.AuthScheme == 0 {
		if len(salt) != e2ee.SaltLen || len(authKey) != e2ee.KeyLen {
			return nil, "", ErrBadKeys
		}
		if err := setAuthKey(ctx, tx, userID, salt, authKey); err != nil {
			return nil, "", err
		}
		credential = string(verifier(authKey))
	}
	stored := *k
	if err := tx.QueryRowContext(ctx, `
		INSERT INTO user_keys (user_id, identity_pub, enc_pub, key_sig, ak_password, ak_recovery, bundle, recovery_verifier)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id`,
		userID, k.IdentityPub, k.EncPub, k.KeySig, k.AKPassword, k.AKRecovery, k.Bundle,
		verifier(recoveryKey)).Scan(&stored.KeyID); err != nil {
		return nil, "", err
	}
	if err := tx.Commit(); err != nil {
		return nil, "", err
	}
	return &stored, credential, nil
}

// ChangePassword replaces salt, verifier and akPassword after checking the old
// auth key, and returns the new credential.
func (s *Store) ChangePassword(ctx context.Context, userID int, oldKey, newSalt, newKey, akPassword []byte) (string, error) {
	if len(newSalt) != e2ee.SaltLen || len(newKey) != e2ee.KeyLen || len(akPassword) != e2ee.WrappedAKLen {
		return "", ErrBadKeys
	}
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return "", err
	}
	defer tx.Rollback()

	user, err := lockCredential(ctx, tx, userID)
	if err != nil {
		return "", err
	}
	if user.AuthScheme != 1 || !matches(user.Verifier, oldKey) {
		return "", ErrBadCredentials
	}
	res, err := tx.ExecContext(ctx, `
		UPDATE user_keys SET ak_password = $2 WHERE user_id = $1 AND retired_at IS NULL`, userID, akPassword)
	if err != nil {
		return "", err
	}
	if n, err := res.RowsAffected(); err != nil || n != 1 {
		return "", errors.Join(ErrBadCredentials, err)
	}
	if err := setAuthKey(ctx, tx, userID, newSalt, newKey); err != nil {
		return "", err
	}
	if err := tx.Commit(); err != nil {
		return "", err
	}
	return string(verifier(newKey)), nil
}

// checkRecovery verifies a recovery auth key. Unknown accounts, accounts
// without keys and wrong keys are all ErrBadCredentials. A 128-bit code needs no
// per-account budget. A suffix of "FOR UPDATE" locks the user row.
func checkRecovery(ctx context.Context, q queryer, identifier, suffix string, key []byte) (int, *e2ee.AccountKeys, error) {
	user, err := findLogin(ctx, q, identifier, suffix)
	if errors.Is(err, ErrNotFound) {
		matches(nil, key)
		return 0, nil, ErrBadCredentials
	}
	if err != nil {
		return 0, nil, err
	}
	keys, stored, err := currentKeys(ctx, q, user.ID)
	if err != nil && !errors.Is(err, ErrNoKeys) {
		return 0, nil, err
	}
	if !matches(stored, key) {
		return 0, nil, ErrBadCredentials
	}
	return user.ID, keys, nil
}

// RecoverBegin returns the keys of an account to the holder of its recovery
// code.
func (s *Store) RecoverBegin(ctx context.Context, identifier string, key []byte) (int, *e2ee.AccountKeys, error) {
	return checkRecovery(ctx, s.db, identifier, "", key)
}

// Recovery is what RECOVER_FINISH replaces.
type Recovery struct {
	Salt, AuthKey, RecoveryAuthKey []byte
	Keys                           *e2ee.AccountKeys // akPassword and akRecovery
}

// RecoverFinish replaces password and recovery code and returns the account's
// ID. The identity stays.
func (s *Store) RecoverFinish(ctx context.Context, identifier string, key []byte, r Recovery) (int, error) {
	if len(r.Salt) != e2ee.SaltLen || len(r.AuthKey) != e2ee.KeyLen || len(r.RecoveryAuthKey) != e2ee.KeyLen ||
		r.Keys == nil || len(r.Keys.AKPassword) != e2ee.WrappedAKLen || len(r.Keys.AKRecovery) != e2ee.WrappedAKLen {
		return 0, ErrBadCredentials
	}
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return 0, err
	}
	defer tx.Rollback()

	// The row lock comes with the lookup; the keys are read after it.
	userID, keys, err := checkRecovery(ctx, tx, identifier, "FOR UPDATE", key)
	if err != nil {
		return 0, err
	}
	if _, err := tx.ExecContext(ctx, `
		UPDATE user_keys SET ak_password = $2, ak_recovery = $3, recovery_verifier = $4 WHERE id = $1`,
		keys.KeyID, r.Keys.AKPassword, r.Keys.AKRecovery, verifier(r.RecoveryAuthKey)); err != nil {
		return 0, err
	}
	if err := setAuthKey(ctx, tx, userID, r.Salt, r.AuthKey); err != nil {
		return 0, err
	}
	return userID, tx.Commit()
}

const resetPrefix = "FEATHER-RESET-"

// Reset is the operator reset: a random temporary password on scheme 0, the
// current keys retired and the user's key wraps deleted. It returns the
// password, which is stored nowhere else.
func (s *Store) Reset(ctx context.Context, username string) (string, error) {
	// The prefix tells a client that scheme 0 is expected. rand.Text is 26
	// base32 characters: well under the 72 bytes bcrypt reads.
	password := resetPrefix + rand.Text()
	hash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return "", err
	}
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return "", err
	}
	defer tx.Rollback()

	var userID int
	err = tx.QueryRowContext(ctx, `
		UPDATE users SET auth_scheme = 0, password_hash = $2, auth_salt = NULL, auth_verifier = NULL, updated_at = now()
		WHERE lower(username) = lower($1) RETURNING id`, username, string(hash)).Scan(&userID)
	if errors.Is(err, sql.ErrNoRows) {
		return "", ErrNotFound
	}
	if err != nil {
		return "", err
	}
	if _, err := tx.ExecContext(ctx, `
		UPDATE user_keys SET retired_at = now(), ak_password = NULL, ak_recovery = NULL, bundle = NULL,
			recovery_verifier = NULL
		WHERE user_id = $1 AND retired_at IS NULL`, userID); err != nil {
		return "", err
	}
	if _, err := tx.ExecContext(ctx, `DELETE FROM epoch_wraps WHERE recipient_id = $1`, userID); err != nil {
		return "", err
	}
	if err := tx.Commit(); err != nil {
		return "", err
	}
	return password, nil
}

// StoredCredential returns the credential now in force, to end sessions that
// logged in against an older one.
func (s *Store) StoredCredential(ctx context.Context, userID int) (string, error) {
	var u User
	err := s.db.QueryRowContext(ctx, `
		SELECT password_hash, auth_scheme, auth_verifier FROM users WHERE id = $1`, userID).
		Scan(&u.PasswordHash, &u.AuthScheme, &u.Verifier)
	if errors.Is(err, sql.ErrNoRows) {
		return "", ErrNotFound
	}
	return u.Credential(), err
}

// FakeSaltSecret returns the random secret behind the PRELOGIN salt of unknown
// identifiers, creating it on first use.
func (s *Store) FakeSaltSecret(ctx context.Context) ([]byte, error) {
	const read = `SELECT fake_salt FROM server_secrets`
	var secret []byte
	err := s.db.QueryRowContext(ctx, read).Scan(&secret)
	if !errors.Is(err, sql.ErrNoRows) {
		return secret, err
	}
	fresh := make([]byte, e2ee.KeyLen)
	_, _ = rand.Read(fresh) // never fails
	if _, err := s.db.ExecContext(ctx, `
		INSERT INTO server_secrets (fake_salt) VALUES ($1) ON CONFLICT DO NOTHING`, fresh); err != nil {
		return nil, err
	}
	err = s.db.QueryRowContext(ctx, read).Scan(&secret)
	return secret, err
}

// UserKeys returns one key set of target (keyID 0: the current one) if viewer
// is target, shares a server or a DM with it, or can access a conversation that
// has an epoch naming it.
func (s *Store) UserKeys(ctx context.Context, viewerID, targetID int, keyID int64) (*e2ee.UserKeys, error) {
	var k e2ee.UserKeys
	err := s.db.QueryRowContext(ctx, `
		SELECT k.user_id, k.id, k.identity_pub, k.enc_pub, k.key_sig, k.retired_at IS NOT NULL
		FROM user_keys k
		WHERE k.user_id = $2::bigint
		  AND (($3::bigint = 0 AND k.retired_at IS NULL) OR k.id = $3::bigint)
		  AND ($1::bigint = $2::bigint
		    OR EXISTS (
				SELECT 1 FROM servers s
				WHERE (s.owner_id = $1::bigint OR EXISTS (SELECT 1 FROM server_members m WHERE m.server_id = s.id
				       AND m.user_id = $1::bigint AND COALESCE(m.banned, FALSE) = FALSE))
				  AND (s.owner_id = $2::bigint OR EXISTS (SELECT 1 FROM server_members m WHERE m.server_id = s.id
				       AND m.user_id = $2::bigint AND COALESCE(m.banned, FALSE) = FALSE)))
		    OR EXISTS (
				SELECT 1 FROM conversations c
				WHERE c.user_low = LEAST($1::bigint, $2::bigint) AND c.user_high = GREATEST($1::bigint, $2::bigint))
		    OR EXISTS (
				SELECT 1 FROM (SELECT DISTINCT conversation_id FROM epoch_recipients WHERE user_id = $2::bigint) r
				WHERE $1::bigint IN (SELECT user_id FROM conversation_member_ids(r.conversation_id))))`,
		viewerID, targetID, keyID).
		Scan(&k.UserID, &k.KeyID, &k.IdentityPub, &k.EncPub, &k.KeySig, &k.Retired)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNoKeys
	}
	if err != nil {
		return nil, err
	}
	return &k, nil
}
