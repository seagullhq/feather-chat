package control

import (
	"errors"
	"log"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

const usernameHasAt = "username cannot contain @"

// loginRefused answers a wrong password and a spent account budget alike: a
// different reply would show that a username and an email share an account.
const loginRefused = "invalid credentials, or too many failed attempts: try again later"

var emailRe = regexp.MustCompile(`^[^@\s]+@[^@\s]+\.[^@\s]+$`)

// validateRegistration checks the REGISTER payload before it reaches the
// store.
func validateRegistration(m protocol.Control) error {
	if n := utf8.RuneCountInString(strings.TrimSpace(m.Username)); n < 3 || n > 32 {
		return errors.New("username must be 3-32 characters")
	}
	if strings.Contains(m.Username, "@") {
		return errors.New(usernameHasAt)
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
	if !s.authByIP.allow(c.ip, time.Now()) {
		c.sendError(tooManyAttempts)
		return
	}
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
	log.Printf("[AUTH] registered user %q", strings.TrimSpace(msg.Username))
	c.send(protocol.Control{Type: "ACK"})
}

// reserveLogin takes a failure token from both buckets of an account: the one
// of this source and the one shared by all sources.
func (s *Server) reserveLogin(account, ip string) (done func(failed bool), ok bool) {
	now := time.Now()
	own, ok := s.failedLogins.reserve(account+"|"+ip, now)
	if !ok {
		return nil, false
	}
	all, ok := s.accountLogins.reserve(account, now)
	if !ok {
		own(false)
		return nil, false
	}
	return func(failed bool) {
		own(failed)
		all(failed)
	}, true
}

// loginGate limits the password attempts of one LOGIN. A device that has logged
// in to the account before spends its own budget, so failures from elsewhere do
// not lock it out.
func (s *Server) loginGate(token, ip string) accounts.Gate {
	return func(userID int, passwordHash string) (func(failed bool), bool) {
		now := time.Now()
		if device, ok := s.checkDeviceToken(token, userID, passwordHash, now); ok {
			return s.failedLogins.reserve("device|"+device, now)
		}
		return s.reserveLogin(strconv.Itoa(userID), ip)
	}
}

// handleLogin answers LOGIN by authenticating and binding the user to the
// session.
func (s *Server) handleLogin(c *client, msg protocol.Control) {
	if !s.authByIP.allow(c.ip, time.Now()) {
		c.sendError(tooManyAttempts)
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	user, err := s.users.Authenticate(ctx, msg.Identifier, msg.Password, s.loginGate(msg.DeviceToken, c.ip))
	if err != nil {
		if !errors.Is(err, accounts.ErrBadCredentials) {
			log.Printf("[AUTH] login failed: %v", err)
		}
		c.sendError(loginRefused)
		return
	}
	s.rooms.Bind(c.sess, user.ID, user.Username)
	s.conns.promote(c.slot)
	log.Printf("[AUTH] user %d (%q) logged in", user.ID, user.Username)
	c.send(protocol.Control{
		Type: "ACK", Name: user.Username,
		DeviceToken: s.issueDeviceToken(user.ID, user.PasswordHash, time.Now()),
	})
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
	if strings.Contains(username, "@") {
		c.sendError(usernameHasAt) // it could pass for someone else's email
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

	s.rooms.Bind(c.sess, c.sess.UserID, user.Username)
	c.send(protocol.Control{Type: "ACK", Name: user.Username})
}

// handleChangePassword answers UPDATE_PASSWORD after checking the current
// password.
func (s *Server) handleChangePassword(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if !s.authByIP.allow(c.ip, time.Now()) {
		c.sendError(tooManyAttempts)
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
		return // a second reply would answer the client's next request
	}
	c.send(protocol.Control{Type: "ACK"})
}
