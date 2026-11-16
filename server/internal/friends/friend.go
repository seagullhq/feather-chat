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
	ErrRemoveFriend   = errors.New("could not remove the friendship")
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


// RemoveFriendship ends the friendship between userID and friendID, whoever
// asked first. Only ACCEPTED rows: a blocked user must not lift a block this
// way. ErrRemoveFriend when they are not friends.
func (s *ServerStore) RemoveFriendship(ctx context.Context, userID, friendID int) error {
	res, err := s.db.ExecContext(ctx, `
		DELETE FROM friendships
		WHERE status = 'ACCEPTED'
		  AND ((requester_id = $1 AND addressee_id = $2)
		    OR (requester_id = $2 AND addressee_id = $1))`,
		userID, friendID)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err != nil {
		return err
	} else if n == 0 {
		return ErrRemoveFriend
	}
	return nil
}


// DeclineFriendship drops the pending request from requesterID to userID.
// ponytail: deletes the row (no REJECTED), so they can ask again later.
func (s *ServerStore) DeclineFriendship(ctx context.Context, userID, requesterID int) error {
	res, err := s.db.ExecContext(ctx, `
		DELETE FROM friendships
		WHERE requester_id = $2 AND addressee_id = $1 AND status = 'PENDING'`,
		userID, requesterID)
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

// CancelFriendship withdraws the pending request requesterID sent to addresseeID.
// ErrNotFound when requesterID has no pending request to that user.
func (s *ServerStore) CancelFriendship(ctx context.Context, requesterID, addresseeID int) error {
	res, err := s.db.ExecContext(ctx, `
		DELETE FROM friendships
		WHERE requester_id = $1 AND addressee_id = $2 AND status = 'PENDING'`,
		requesterID, addresseeID)
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

// BlockFriendship makes userID the blocker of blockedID. Any row between the
// pair is replaced by a BLOCKED row whose requester is the blocker (see the
// friendships schema). The unique pair index then also stops blockedID from
// sending a request.
func (s *ServerStore) BlockFriendship(ctx context.Context, userID, blockedID int) error {
	if userID == blockedID {
		return ErrSelfRequest
	}
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()

	if _, err := tx.ExecContext(ctx, `
		DELETE FROM friendships
		WHERE (requester_id = $1 AND addressee_id = $2)
		   OR (requester_id = $2 AND addressee_id = $1)`,
		userID, blockedID); err != nil {
		return err
	}
	now := time.Now().UTC()
	if _, err := tx.ExecContext(ctx, `
		INSERT INTO friendships (requester_id, addressee_id, status, created_at, updated_at)
		VALUES ($1, $2, 'BLOCKED', $3, $3)`,
		userID, blockedID, now); err != nil {
		return err
	}
	return tx.Commit()
}

// UnblockFriendship removes the block userID placed on blockedID. The pair goes
// back to nobody in particular: the old friendship does not come back, and
// either side can send a new request. ErrNotFound when userID blocks nobody there.
func (s *ServerStore) UnblockFriendship(ctx context.Context, userID, blockedID int) error {
	res, err := s.db.ExecContext(ctx, `
		DELETE FROM friendships
		WHERE status = 'BLOCKED' AND requester_id = $1 AND addressee_id = $2`,
		userID, blockedID)
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
// "incoming" (they asked you), "outgoing" (you asked them), "blocked" (you
// blocked them; only from FetchRequests) or "" for nobody in particular.
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
	// A BLOCKED row counts only for its blocker, the requester (see BlockFriendship).
	rows, err := s.db.QueryContext(ctx, `
		SELECT u.id, u.username,
		  CASE WHEN f.status = 'BLOCKED' THEN 'blocked' ELSE (`+relationSQL+`) END
		FROM friendships f
		JOIN users u ON u.id = CASE WHEN f.requester_id = $1 THEN f.addressee_id ELSE f.requester_id END
		WHERE (f.requester_id = $1 OR f.addressee_id = $1)
		  AND (f.status IN ('PENDING', 'ACCEPTED')
		       OR (f.status = 'BLOCKED' AND f.requester_id = $1))
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
