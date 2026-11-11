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

// UpdateServer renames a server, but only when userID owns it.
func (s *ServerStore) UpdateServer(ctx context.Context, userID int, serverID int, name string) (*Server, error) {
	var srv Server
	err := s.db.QueryRowContext(ctx, `
		UPDATE servers
		SET name = $1, updated_at = now()
		WHERE id = $2 AND owner_id = $3
		RETURNING id, name, invite_code
	`, name, serverID, userID).Scan(&srv.ID, &srv.Name, &srv.InviteCode)
	if err == nil {
		return &srv, nil
	}

	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == "23505" { // unique_violation
		return nil, ErrTaken
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}

	// No row updated: missing server or caller isn't the owner
	var exists bool
	if err := s.db.QueryRowContext(ctx,
		`SELECT EXISTS(SELECT 1 FROM servers WHERE id = $1)`, serverID,
	).Scan(&exists); err != nil {
		return nil, err
	}
	if !exists {
		return nil, ErrNotFound
	}
	return nil, ErrForbidden
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
