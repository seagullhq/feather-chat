// Package friends request
package friends

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
)

var (
	ErrTaken          = errors.New("username or email already registered")
	ErrBadCredentials = errors.New("invalid credentials")
	ErrNotFound       = errors.New("user not found")
	ErrSelfRequest    = errors.New("cannot send a friend request to yourself")
	ErrAlreadyExists  = errors.New("friend request already exists")
)

type User struct {
	ID           int       `json:"id"`
	Username     string    `json:"username"`
	Email        string    `json:"email"`
	PasswordHash string    `json:"-"`
	CreatedAt    time.Time `json:"created_at"`
	UpdatedAt    time.Time `json:"updated_at"`
}

// ServerStore persists servers and their content.
type ServerStore struct {
	db *sql.DB
}

// NewStore wires a ServerStore onto an open database.
func NewStore(db *sql.DB) *ServerStore {
	return &ServerStore{db: db}
}

/*
 CREATE TABLE IF NOT EXISTS friendships (
     -- Who asked; for a BLOCKED row, who blocked.
     requester_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
     addressee_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

     status VARCHAR(20) NOT NULL DEFAULT 'PENDING'
         CHECK (status IN ('PENDING', 'ACCEPTED', 'REJECTED', 'BLOCKED')),

     created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
     updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),

     PRIMARY KEY (requester_id, addressee_id),
     CHECK (requester_id <> addressee_id)
 );

 -- One row per pair, whoever asked first.
 CREATE UNIQUE INDEX friendships_pair_idx
     ON friendships (LEAST(requester_id, addressee_id), GREATEST(requester_id, addressee_id));
 CREATE INDEX friendships_addressee_idx ON friendships (addressee_id);

*/

type FriendRequest struct {
	RequesterID int       `json:"requester_id"`
	AddresseeID int       `json:"addressee_id"`
	Status      string    `json:"status"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

// SendFriendship stores a pending friend request from requesterID to
// addresseeID. The pair index makes a second request, in either direction, fail
// with ErrAlreadyExists.
func (s *ServerStore) SendFriendship(ctx context.Context, requesterID, addresseeID int) (*FriendRequest, error) {
	if requesterID == addresseeID {
		return nil, ErrSelfRequest
	}

	query := `
		INSERT INTO friendships (requester_id, addressee_id, status, created_at, updated_at)
		VALUES ($1, $2, 'PENDING', $3, $4)
		RETURNING status, created_at, updated_at
	`

	now := time.Now().UTC()

	request := FriendRequest{
		RequesterID: requesterID,
		AddresseeID: addresseeID,
	}
	err := s.db.QueryRowContext(ctx, query, requesterID, addresseeID, now, now).
		Scan(&request.Status, &request.CreatedAt, &request.UpdatedAt)
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) {
			switch pgErr.Code {
			case "23505": // unique_violation: request already exists for this pair
				return nil, ErrAlreadyExists
			case "23514": // check_violation: requester and addressee are the same
				return nil, ErrSelfRequest
			case "23503": // foreign_key_violation: one of the users is gone
				return nil, ErrNotFound
			}
		}
		return nil, err
	}

	return &request, nil
}
