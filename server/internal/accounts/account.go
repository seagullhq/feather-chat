// Package accounts registers and authenticates users and keeps their keys on
// PostgreSQL: auth key verifiers (or legacy bcrypt hashes), never passwords.
package accounts

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"database/sql"
	"errors"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"golang.org/x/crypto/bcrypt"

	"github.com/seagullhq/feather-chat/server/internal/e2ee"
)

var (
	ErrTaken          = errors.New("username or email already registered")
	ErrBadCredentials = errors.New("invalid credentials")
	ErrNotFound       = errors.New("user not found")
)

type User struct {
	ID           int       `json:"id"`
	Username     string    `json:"username"`
	Email        string    `json:"email"`
	PasswordHash string    `json:"-"`
	CreatedAt    time.Time `json:"created_at"`
	UpdatedAt    time.Time `json:"updated_at"`
	// AuthScheme is 0 (bcrypt PasswordHash) or 1 (Salt and Verifier). These
	// three are set by Authenticate only.
	AuthScheme int    `json:"-"`
	Salt       []byte `json:"-"`
	Verifier   []byte `json:"-"`
}

// Credential is the stored secret of the account's scheme. Device tokens cover
// it.
func (u *User) Credential() string {
	if u.AuthScheme == 1 {
		return string(u.Verifier)
	}
	return u.PasswordHash
}

// Secret is what a LOGIN proves: a legacy password or an auth key, not both.
type Secret struct {
	Password string
	AuthKey  []byte
}

// verifier is what the server stores of an auth key.
func verifier(key []byte) []byte {
	sum := sha256.Sum256(key)
	return sum[:]
}

// matches compares in constant time.
func matches(stored, key []byte) bool {
	same := subtle.ConstantTimeCompare(stored, verifier(key)) == 1
	return same && len(key) == e2ee.KeyLen
}

type Store struct {
	db *sql.DB
}

func NewStore(db *sql.DB) *Store {
	return &Store{db: db}
}

var dummyHash, _ = bcrypt.GenerateFromPassword([]byte("feather-dummy"), bcrypt.DefaultCost)

// Register creates an account on auth scheme 1, without keys.
func (s *Store) Register(ctx context.Context, username, email string, salt, authKey []byte) error {
	if len(salt) != e2ee.SaltLen || len(authKey) != e2ee.KeyLen {
		return ErrBadCredentials
	}
	_, err := s.db.ExecContext(ctx, `
		INSERT INTO users (username, email, password_hash, auth_scheme, auth_salt, auth_verifier)
		VALUES ($1, $2, '', 1, $3, $4)`,
		strings.TrimSpace(username), strings.TrimSpace(email), salt, verifier(authKey))
	if err != nil {
		// Check for a duplicate key error (Postgres code 23505)
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return ErrTaken
		}
		return err
	}
	return nil
}

// Delete removes the account after checking its auth key under the gate.
func (s *Store) Delete(ctx context.Context, userID int, authKey []byte, gate Gate) error {
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return err
	}
	defer tx.Rollback()

	user, err := lockCredential(ctx, tx, userID)
	if err != nil {
		return err
	}
	done, ok := gate(userID, user.Credential())
	if !ok {
		return ErrBadCredentials
	}
	right := user.AuthScheme == 1 && matches(user.Verifier, authKey)
	done(!right)
	if !right {
		return ErrBadCredentials
	}
	if _, err := tx.ExecContext(ctx, `DELETE FROM users WHERE id = $1`, userID); err != nil {
		return err
	}
	return tx.Commit()
}

// Gate limits login attempts per account. It is asked before the secret is
// compared; if ok, done must be called once with whether the secret was wrong.
type Gate func(userID int, credential string) (done func(failed bool), ok bool)

// findLogin returns the account named by identifier (username or email).
func findLogin(ctx context.Context, q queryer, identifier, suffix string) (*User, error) {
	var user User
	err := q.QueryRowContext(ctx, `
		SELECT id, username, email, password_hash, created_at, updated_at, auth_scheme, auth_salt, auth_verifier
		FROM users
		WHERE lower(username) = lower($1) OR lower(email) = lower($1)
		ORDER BY (lower(email) = lower($1)) DESC
		LIMIT 1 `+suffix, strings.TrimSpace(identifier)).Scan(
		&user.ID, &user.Username, &user.Email, &user.PasswordHash, &user.CreatedAt, &user.UpdatedAt,
		&user.AuthScheme, &user.Salt, &user.Verifier)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &user, nil
}

// Prelogin returns the auth scheme of an account and, for scheme 1, its salt.
func (s *Store) Prelogin(ctx context.Context, identifier string) (int, []byte, error) {
	user, err := findLogin(ctx, s.db, identifier, "")
	if err != nil {
		return 0, nil, err
	}
	if user.AuthScheme == 0 {
		return 0, nil, nil
	}
	return 1, user.Salt, nil
}

// Authenticate returns the user named by identifier (username or email) if the
// secret matches the account's scheme. A refusal by a gate, or a secret of the
// other scheme, looks like an unknown identifier: one compare against nothing
// and ErrBadCredentials, so it reveals nothing about accounts.
func (s *Store) Authenticate(ctx context.Context, identifier string, secret Secret, gates ...Gate) (*User, error) {
	legacy := secret.Password != ""
	if legacy && len(secret.AuthKey) != 0 {
		return nil, ErrBadCredentials
	}
	dummy := func() {
		if legacy {
			_ = bcrypt.CompareHashAndPassword(dummyHash, []byte(secret.Password))
		} else {
			matches(make([]byte, sha256.Size), secret.AuthKey)
		}
	}

	user, err := findLogin(ctx, s.db, identifier, "")
	if errors.Is(err, ErrNotFound) {
		dummy()
		return nil, ErrBadCredentials
	}
	if err != nil {
		return nil, err
	}

	var dones []func(failed bool)
	allowed := true
	for _, gate := range gates {
		done, ok := gate(user.ID, user.Credential())
		if !ok {
			allowed = false
			break
		}
		dones = append(dones, done)
	}
	var right bool
	switch {
	case !allowed || legacy != (user.AuthScheme == 0):
		dummy()
	case legacy:
		right = bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(secret.Password)) == nil
	default:
		right = matches(user.Verifier, secret.AuthKey)
	}
	for _, done := range dones {
		done(allowed && !right)
	}
	if !right {
		return nil, ErrBadCredentials
	}

	return user, nil
}

// FindByUsername returns the user with the given username, case-insensitively.
func (s *Store) FindByUsername(ctx context.Context, username string) (*User, error) {
	var user User
	err := s.db.QueryRowContext(ctx, `
		SELECT id, username, email, password_hash, created_at, updated_at
		FROM users
		WHERE lower(username) = lower($1)
	`, strings.TrimSpace(username)).Scan(
		&user.ID,
		&user.Username,
		&user.Email,
		&user.PasswordHash,
		&user.CreatedAt,
		&user.UpdatedAt,
	)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &user, nil
}

// UpdateUsername changes the username of userID and returns the refreshed user.
func (s *Store) UpdateUsername(ctx context.Context, userID int, username string) (*User, error) {
	query := `
		UPDATE users
		SET username = $1, updated_at = now()
		WHERE id = $2
		RETURNING id, username, email, password_hash, created_at, updated_at
	`

	var user User
	err := s.db.QueryRowContext(ctx, query, strings.TrimSpace(username), userID).Scan(
		&user.ID,
		&user.Username,
		&user.Email,
		&user.PasswordHash,
		&user.CreatedAt,
		&user.UpdatedAt,
	)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return nil, ErrTaken
		}
		return nil, err
	}

	return &user, nil
}
