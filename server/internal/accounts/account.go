// Package accounts registers and authenticates users, storing bcrypt
// password hashes on PostgreSQL.
package accounts

import (
	"context"
	"database/sql"
	"errors"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"golang.org/x/crypto/bcrypt"
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
}

type Store struct {
	db *sql.DB
}

func NewStore(db *sql.DB) *Store {
	return &Store{db: db}
}

var dummyHash, _ = bcrypt.GenerateFromPassword([]byte("feather-dummy"), bcrypt.DefaultCost)

func (s *Store) Register(ctx context.Context, username, email, password string) error {
	hash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return err
	}
	now := time.Now()

	query := `
		INSERT INTO users (username, email, password_hash, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5)
	`

	_, err = s.db.ExecContext(ctx, query, strings.TrimSpace(username), strings.TrimSpace(email), string(hash), now, now)

	if err != nil {
		// Controllo errore di chiave duplicata (codice Postgres 23505)
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return ErrTaken
		}
		return err
	}
	return nil
}

func (s *Store) Delete(ctx context.Context, userID int) error {
	const query = `DELETE FROM users WHERE id = $1`

	res, err := s.db.ExecContext(ctx, query, userID)
	if err != nil {
		return err
	}

	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n == 0 {
		return ErrNotFound
	}

	return nil
}

func (s *Store) Authenticate(ctx context.Context, identifier string, password string) (*User, error) {
	identifier = strings.TrimSpace(identifier)

	query := `
		SELECT id, username, email, password_hash, created_at, updated_at
		FROM users
		WHERE lower(username) = lower($1) OR lower(email) = lower($1)
	`

	var user User
	err := s.db.QueryRowContext(ctx, query, identifier).Scan(
		&user.ID,
		&user.Username,
		&user.Email,
		&user.PasswordHash,
		&user.CreatedAt,
		&user.UpdatedAt,
	)

	if errors.Is(err, sql.ErrNoRows) {
		_ = bcrypt.CompareHashAndPassword(dummyHash, []byte(password))
		return nil, ErrBadCredentials
	}
	if err != nil {
		return nil, err
	}

	if bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(password)) != nil {
		return nil, ErrBadCredentials
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

// ChangePassword replaces the password of userID after checking oldPassword.
func (s *Store) ChangePassword(ctx context.Context, userID int, oldPassword, newPassword string) error {
	var stored string
	err := s.db.QueryRowContext(ctx, `SELECT password_hash FROM users WHERE id = $1`, userID).Scan(&stored)
	if errors.Is(err, sql.ErrNoRows) {
		return ErrNotFound
	}
	if err != nil {
		return err
	}
	if bcrypt.CompareHashAndPassword([]byte(stored), []byte(oldPassword)) != nil {
		return ErrBadCredentials
	}

	hash, err := bcrypt.GenerateFromPassword([]byte(newPassword), bcrypt.DefaultCost)
	if err != nil {
		return err
	}

	_, err = s.db.ExecContext(ctx,
		`UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2`,
		string(hash), userID,
	)
	return err
}
