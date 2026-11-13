// Package servers stores the chat servers, their channels and their members
// on PostgreSQL, enforcing every visibility rule in SQL.
package servers

import (
	"database/sql"
	"errors"
)

// Errors surfaced to the control plane and, through it, to clients.
var (
	ErrTaken        = errors.New("server name already taken")
	ErrBadInvite    = errors.New("invalid invite code")
	ErrBanned       = errors.New("banned from this server")
	ErrForbidden    = errors.New("forbidden")
	ErrCantDelete   = errors.New("only the owner can delete this server")
	ErrFailedDelete = errors.New("server deletion failed")
)

// ServerStore persists servers and their content.
type ServerStore struct {
	db *sql.DB
}

// NewStore wires a ServerStore onto an open database.
func NewStore(db *sql.DB) *ServerStore {
	return &ServerStore{db: db}
}
