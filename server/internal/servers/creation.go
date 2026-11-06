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

var ErrTaken = errors.New("server already exists")

type ServerStore struct {
	db *sql.DB
}

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

func NewStore(db *sql.DB) *ServerStore {
	return &ServerStore{db: db}
}

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
