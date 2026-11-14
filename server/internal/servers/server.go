package servers

import (
	"context"
	"crypto/rand"
	"database/sql"
	"errors"
	"math/big"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
)

// Server is one chat server row.
type Server struct {
	ID         int       `json:"id"`
	Name       string    `json:"name"`
	OwnerID    int       `json:"owner_id"`
	Members    []int     `json:"members,omitempty"`
	InviteCode string    `json:"invite_code"`
	CreatedAt  time.Time `json:"created_at"`
	UpdatedAt  time.Time `json:"updated_at"`
}

const inviteCharset = "abcdefghijklmnopqrstuvwxyz" +
	"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"

// inviteCode rolls a cryptographically random code of the given length.
func inviteCode(length int) (string, error) {
	b := make([]byte, length)
	limit := big.NewInt(int64(len(inviteCharset)))
	for i := range b {
		n, err := rand.Int(rand.Reader, limit)
		if err != nil {
			return "", err
		}
		b[i] = inviteCharset[n.Int64()]
	}
	return string(b), nil
}

type queryer interface {
	QueryRowContext(ctx context.Context, query string, args ...any) *sql.Row
}

// isAdmin reports whether userID is a non-banned member holding ADMIN on serverID.
func isAdmin(ctx context.Context, q queryer, serverID, userID int) (bool, error) {
	var ok bool
	err := q.QueryRowContext(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM server_permissions sp
			JOIN permission_levels pl ON pl.id = sp.perm_level
			JOIN server_members m ON m.server_id = sp.server_id AND m.user_id = sp.user_id
			WHERE sp.server_id = $1 AND sp.user_id = $2
			  AND pl.permission_id = 'ADMIN'
			  AND COALESCE(m.banned, false) = false
		)`, serverID, userID).Scan(&ok)
	return ok, err
}

// canManage allows the owner and the admins of a server (rename, new channels).
func canManage(ctx context.Context, q queryer, serverID, userID int) error {
	var ownerID int
	if err := q.QueryRowContext(ctx,
		`SELECT owner_id FROM servers WHERE id = $1`, serverID,
	).Scan(&ownerID); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ErrNotFound
		}
		return err
	}
	if userID == ownerID {
		return nil
	}
	if ok, err := isAdmin(ctx, q, serverID, userID); err != nil {
		return err
	} else if !ok {
		return ErrForbidden
	}
	return nil
}

// canModerate says whether requesterID may kick or ban targetUserID. The owner
// may act on anyone but themselves-as-owner; an admin only on plain members
// (not the owner, not other admins). Everyone else may not.
func canModerate(ctx context.Context, tx *sql.Tx, serverID, ownerID, requesterID, targetUserID int) error {
	if targetUserID == ownerID {
		return ErrCantDelete
	}
	if requesterID == ownerID {
		return nil
	}
	if ok, err := isAdmin(ctx, tx, serverID, requesterID); err != nil {
		return err
	} else if !ok {
		return ErrCantDelete
	}
	if ok, err := isAdmin(ctx, tx, serverID, targetUserID); err != nil {
		return err
	} else if ok {
		return ErrCantDelete
	}
	return nil
}

// KickUser removes a user from a server.
func (s *ServerStore) KickUser(ctx context.Context, requesterID int, serverID int, targetUserID int) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()

	var ownerID int
	err = tx.QueryRowContext(ctx,
		`SELECT owner_id FROM servers WHERE id = $1 FOR UPDATE`,
		serverID,
	).Scan(&ownerID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ErrNotFound
		}
		return err
	}

	if err := canModerate(ctx, tx, serverID, ownerID, requesterID, targetUserID); err != nil {
		return err
	}

	res, err := tx.ExecContext(ctx,
		`DELETE FROM server_members WHERE server_id = $1 AND user_id = $2`,
		serverID, targetUserID,
	)
	if err != nil {
		return err
	}

	rows, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if rows == 0 {
		return ErrNotFound
	}

	return tx.Commit()
}

// UnbanUser lifts a ban. The owner and the admins may; the user must be
// currently banned from the server.
func (s *ServerStore) UnbanUser(ctx context.Context, requesterID int, serverID int, targetUserID int) error {
	if err := canManage(ctx, s.db, serverID, requesterID); err != nil {
		return err
	}
	res, err := s.db.ExecContext(ctx,
		`UPDATE server_members SET banned = false
		WHERE server_id = $1 AND user_id = $2 AND COALESCE(banned, false) = true`,
		serverID, targetUserID,
	)
	if err != nil {
		return err
	}
	rows, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if rows == 0 {
		return ErrNotFound
	}
	return nil
}

// BanUser bans a user from a server.
func (s *ServerStore) BanUser(ctx context.Context, requesterID int, serverID int, targetUserID int) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()

	var ownerID int
	err = tx.QueryRowContext(ctx,
		`SELECT owner_id FROM servers WHERE id = $1 FOR UPDATE`,
		serverID,
	).Scan(&ownerID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ErrNotFound
		}
		return err
	}

	if err := canModerate(ctx, tx, serverID, ownerID, requesterID, targetUserID); err != nil {
		return err
	}

	res, err := tx.ExecContext(ctx,
		`UPDATE server_members
		SET banned = true
		WHERE server_id = $1 AND user_id = $2;`,
		serverID, targetUserID,
	)
	if err != nil {
		return err
	}

	rows, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if rows == 0 {
		return ErrNotFound
	}

	return tx.Commit()
}

// MakeAdmin grants the ADMIN permission to a member of a server. Only the
// owner may promote someone, and the owner already outranks every role.
func (s *ServerStore) MakeAdmin(ctx context.Context, requesterID int, serverID int, targetUserID int) error {
	return s.changeRole(ctx, requesterID, serverID, targetUserID, true)
}

// MakeMember revokes the member's ADMIN permission. Only the owner may demote
// someone, and the owner is always above every role.
func (s *ServerStore) MakeMember(ctx context.Context, requesterID int, serverID int, targetUserID int) error {
	return s.changeRole(ctx, requesterID, serverID, targetUserID, false)
}

// changeRole promotes (admin=true) or demotes (admin=false) a member.
func (s *ServerStore) changeRole(ctx context.Context, requesterID int, serverID int, targetUserID int, admin bool) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()

	var ownerID int
	err = tx.QueryRowContext(ctx,
		`SELECT owner_id FROM servers WHERE id = $1 FOR UPDATE`,
		serverID,
	).Scan(&ownerID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ErrNotFound
		}
		return err
	}

	if requesterID != ownerID {
		return ErrForbidden
	}

	if targetUserID == ownerID {
		return ErrForbidden
	}

	var isMember bool
	if err := tx.QueryRowContext(ctx,
		`SELECT EXISTS(
			SELECT 1 FROM server_members
			WHERE server_id = $1 AND user_id = $2
			  AND COALESCE(banned, false) = false
		)`, serverID, targetUserID,
	).Scan(&isMember); err != nil {
		return err
	}
	if !isMember {
		return ErrNotFound
	}

	// Keep a single permission row per member, so repeating a role is a no-op.
	if _, err := tx.ExecContext(ctx,
		`DELETE FROM server_permissions WHERE server_id = $1 AND user_id = $2`,
		serverID, targetUserID,
	); err != nil {
		return err
	}

	if admin {
		var permID int
		err = tx.QueryRowContext(ctx,
			`SELECT id FROM permission_levels WHERE permission_id = 'ADMIN'`,
		).Scan(&permID)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				return ErrNotFound
			}
			return err
		}
		if _, err := tx.ExecContext(ctx,
			`INSERT INTO server_permissions (perm_level, user_id, server_id)
			 VALUES ($1, $2, $3)`,
			permID, targetUserID, serverID,
		); err != nil {
			return err
		}
	}

	return tx.Commit()
}

// DeleteServer removes a server, but only when userID owns it.
func (s *ServerStore) DeleteServer(ctx context.Context, userID int, serverID int) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()

	var ownerID int
	err = tx.QueryRowContext(ctx,
		`SELECT owner_id FROM servers WHERE id = $1 FOR UPDATE`,
		serverID,
	).Scan(&ownerID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ErrNotFound
		}
		return err
	}

	if userID != ownerID {
		return ErrCantDelete
	}

	if _, err := tx.ExecContext(ctx,
		`DELETE FROM servers WHERE id = $1`, serverID,
	); err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23503" { // foreign_key_violation
			return ErrFailedDelete
		}
		return err
	}

	return tx.Commit()
}

// UpdateServer renames a server. The owner and the admins may.
func (s *ServerStore) UpdateServer(ctx context.Context, userID int, serverID int, name string) (*Server, error) {
	if err := canManage(ctx, s.db, serverID, userID); err != nil {
		return nil, err
	}
	var srv Server
	err := s.db.QueryRowContext(ctx, `
		UPDATE servers
		SET name = $1, updated_at = now()
		WHERE id = $2
		RETURNING id, name, invite_code
	`, name, serverID).Scan(&srv.ID, &srv.Name, &srv.InviteCode)
	if err == nil {
		return &srv, nil
	}
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == "23505" { // unique_violation
		return nil, ErrTaken
	}
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	return nil, err
}

// CreateServer inserts a server plus its owner's membership in one
// transaction.
func (s *ServerStore) CreateServer(ctx context.Context, ownerID int, serverName string) (*Server, error) {
	code, err := inviteCode(7)
	if err != nil {
		return nil, err
	}

	now := time.Now().UTC()

	// Usiamo una transazione per inserire sia il server che l'associazione del membro
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()

	query := `
		INSERT INTO servers (name, owner_id, invite_code, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5)
		RETURNING id
	`

	var serverID int
	err = tx.QueryRowContext(ctx, query, serverName, ownerID, code, now, now).Scan(&serverID)
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return nil, ErrTaken
		}
		return nil, err
	}

	// Inserimento del proprietario nella tabella dei membri del server
	memberQuery := `
		INSERT INTO server_members (server_id, user_id, joined_at)
		VALUES ($1, $2, $3)
	`
	if _, err = tx.ExecContext(ctx, memberQuery, serverID, ownerID, now); err != nil {
		return nil, err
	}

	if err := tx.Commit(); err != nil {
		return nil, err
	}

	server := Server{
		ID:         serverID,
		Name:       serverName,
		OwnerID:    ownerID,
		Members:    []int{ownerID},
		InviteCode: code,
		CreatedAt:  now,
		UpdatedAt:  now,
	}

	return &server, nil
}

// JoinServer adds the user to the server that owns code. Joining twice is a no-op.
func (s *ServerStore) JoinServer(ctx context.Context, userID int, code string) (*Server, error) {
	var srv Server
	err := s.db.QueryRowContext(ctx,
		`SELECT id, name, invite_code FROM servers WHERE invite_code = $1`, code,
	).Scan(&srv.ID, &srv.Name, &srv.InviteCode)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrBadInvite
	}
	if err != nil {
		return nil, err
	}
	var banned bool
	err = s.db.QueryRowContext(ctx,
		`SELECT COALESCE(banned, false) FROM server_members
		 WHERE server_id = $1 AND user_id = $2`, srv.ID, userID,
	).Scan(&banned)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	if banned {
		return nil, ErrBanned
	}
	if _, err := s.db.ExecContext(ctx, `
		INSERT INTO server_members (server_id, user_id) VALUES ($1, $2)
		ON CONFLICT DO NOTHING`, srv.ID, userID,
	); err != nil {
		return nil, err
	}
	return &srv, nil
}
