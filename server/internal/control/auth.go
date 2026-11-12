package control

import (
	"errors"
	"log"
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

var emailRe = regexp.MustCompile(`^[^@\s]+@[^@\s]+\.[^@\s]+$`)

// validateRegistration checks the REGISTER payload before it reaches the
// store.
func validateRegistration(m protocol.Control) error {
	if n := utf8.RuneCountInString(strings.TrimSpace(m.Username)); n < 3 || n > 32 {
		return errors.New("username must be 3-32 characters")
	}
	if n := len([]byte(m.Password)); n < 8 || n > 72 {
		return errors.New("password must be 8-72 bytes")
	}
	if !emailRe.MatchString(strings.TrimSpace(m.Email)) {
		return errors.New("invalid email")
	}
	return nil
}

// handleRegister answers REGISTER by creating the account.
func (s *Server) handleRegister(c *client, msg protocol.Control) {
	if err := validateRegistration(msg); err != nil {
		c.sendError(err.Error())
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	err := s.users.Register(ctx, msg.Username, msg.Email, msg.Password)
	if errors.Is(err, accounts.ErrTaken) {
		c.sendError(accounts.ErrTaken.Error())
		return
	}
	if err != nil {
		log.Printf("[AUTH] register failed: %v", err) // never log the password
		c.sendError("registration failed")
		return
	}
	c.send(protocol.Control{Type: "ACK"})
}

// handleLogin answers LOGIN by authenticating and binding the user to the
// session.
func (s *Server) handleLogin(c *client, msg protocol.Control) {
	ctx, cancel := storeCtx()
	defer cancel()
	user, err := s.users.Authenticate(ctx, msg.Identifier, msg.Password)
	if err != nil {
		if !errors.Is(err, accounts.ErrBadCredentials) {
			log.Printf("[AUTH] login failed: %v", err)
		}
		c.sendError(accounts.ErrBadCredentials.Error())
		return
	}
	c.sess.UserID = user.ID
	c.sess.Username = user.Username
	c.send(protocol.Control{Type: "ACK", Name: user.Username})
}

// handleUpdateUsername answers UPDATE_USERNAME by renaming the session user.
func (s *Server) handleUpdateUsername(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	username := strings.TrimSpace(msg.Username)
	if n := utf8.RuneCountInString(username); n < 3 || n > 32 {
		c.sendError("username must be 3-32 characters")
		return
	}

	ctx, cancel := storeCtx()
	defer cancel()
	user, err := s.users.UpdateUsername(ctx, c.sess.UserID, username)
	if err != nil {
		switch {
		case errors.Is(err, accounts.ErrTaken), errors.Is(err, accounts.ErrNotFound):
			c.sendError(err.Error())
		default:
			log.Printf("[UPDATE] failed to rename user %d: %v", c.sess.UserID, err)
			c.sendError("username update failed")
		}
		return
	}

	c.sess.Username = user.Username
	c.send(protocol.Control{Type: "ACK", Name: user.Username})
}

// handleChangePassword answers UPDATE_PASSWORD after checking the current
// password.
func (s *Server) handleChangePassword(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.OldPassword == "" {
		c.sendError("current password is required")
		return
	}
	if n := len([]byte(msg.Password)); n < 8 || n > 72 {
		c.sendError("password must be 8-72 bytes")
		return
	}

	ctx, cancel := storeCtx()
	defer cancel()
	err := s.users.ChangePassword(ctx, c.sess.UserID, msg.OldPassword, msg.Password)
	if err != nil {
		switch {
		case errors.Is(err, accounts.ErrBadCredentials), errors.Is(err, accounts.ErrNotFound):
			c.sendError(err.Error())
		default:
			log.Printf("[UPDATE] failed to change password for user %d: %v", c.sess.UserID, err)
			c.sendError("password update failed")
		}
		return
	}

	c.send(protocol.Control{Type: "ACK"})
}

func (s *Server) handleDeleteUser(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	ctx, cancel := storeCtx()
	err := s.users.Delete(ctx, int(c.sess.UserID))
	cancel()

	if err != nil {
		log.Printf("[DELETE] failed to delete user %d: %v", c.sess.UserID, err)
		c.sendError("member deletion failed")
	}
	c.send(protocol.Control{Type: "ACK"})
}
