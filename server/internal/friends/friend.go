// Package friends request
package friends

import (
	"context"
	"database/sql"
	"errors"
	"strings"
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

// AcceptFriendship turns the pending request from requesterID to userID into
// a friendship. ErrNotFound when there is no such pending request.
func (s *ServerStore) AcceptFriendship(ctx context.Context, userID, requesterID int) error {
	res, err := s.db.ExecContext(ctx, `
		UPDATE friendships SET status = 'ACCEPTED', updated_at = $3
		WHERE requester_id = $2 AND addressee_id = $1 AND status = 'PENDING'`,
		userID, requesterID, time.Now().UTC())
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err != nil {
		return err
	} else if n == 0 {
		return ErrNotFound
	}
	return nil
}

// Person is another user as seen by the asking user. Relation is "friend",
// "incoming" (they asked you), "outgoing" (you asked them) or "" for nobody
// in particular.
type Person struct {
	ID       int
	Username string
	Relation string
}

// relationSQL derives Person.Relation from a friendships row f (NULL when the
// pair has no row) for the asking user $1. REJECTED and BLOCKED rows are not
// shown as a relation.
const relationSQL = `CASE
	WHEN f.status = 'ACCEPTED' THEN 'friend'
	WHEN f.status = 'PENDING' AND f.requester_id = $1 THEN 'outgoing'
	WHEN f.status = 'PENDING' THEN 'incoming'
	ELSE '' END`

func scanPeople(rows *sql.Rows) ([]Person, error) {
	defer rows.Close()
	var people []Person
	for rows.Next() {
		var p Person
		if err := rows.Scan(&p.ID, &p.Username, &p.Relation); err != nil {
			return nil, err
		}
		people = append(people, p)
	}
	return people, rows.Err()
}

// FetchRequests lists the friends and pending requests (both directions) of
// userID.
func (s *ServerStore) FetchRequests(ctx context.Context, userID int) ([]Person, error) {
	rows, err := s.db.QueryContext(ctx, `
		SELECT u.id, u.username, `+relationSQL+`
		FROM friendships f
		JOIN users u ON u.id = CASE WHEN f.requester_id = $1 THEN f.addressee_id ELSE f.requester_id END
		WHERE (f.requester_id = $1 OR f.addressee_id = $1)
		  AND f.status IN ('PENDING', 'ACCEPTED')
		ORDER BY LOWER(u.username)`, userID)
	if err != nil {
		return nil, err
	}
	return scanPeople(rows)
}

// SearchLimit caps the results of SearchUsers.
const SearchLimit = 20

// SearchUsers finds users whose name contains query (case-insensitive),
// except userID itself, each with its relation to userID.
func (s *ServerStore) SearchUsers(ctx context.Context, userID int, query string) ([]Person, error) {
	// Escape LIKE wildcards so the query is matched literally.
	like := strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`).Replace(query)
	rows, err := s.db.QueryContext(ctx, `
		SELECT u.id, u.username, COALESCE(`+relationSQL+`, '')
		FROM users u
		LEFT JOIN friendships f
		  ON (f.requester_id = $1 AND f.addressee_id = u.id)
		  OR (f.addressee_id = $1 AND f.requester_id = u.id)
		WHERE u.id <> $1 AND u.username ILIKE '%' || $2 || '%'
		ORDER BY LOWER(u.username)
		LIMIT $3`, userID, like, SearchLimit)
	if err != nil {
		return nil, err
	}
	return scanPeople(rows)
}
