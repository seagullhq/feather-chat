package control

import (
	"crypto/hmac"
	"crypto/sha256"
	"errors"
	"log"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/e2ee"
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
	if m.Password != "" {
		return errors.New("password registration is not supported: update the client")
	}
	if len(m.Salt) != e2ee.SaltLen || len(m.AuthKey) != e2ee.KeyLen {
		return errors.New("invalid salt or auth key")
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
	err := s.users.Register(ctx, msg.Username, msg.Email, msg.Salt, msg.AuthKey)
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
	user, err := s.users.Authenticate(ctx, msg.Identifier,
		accounts.Secret{Password: msg.Password, AuthKey: msg.AuthKey}, s.loginGate(msg.DeviceToken, c.ip))
	if err != nil {
		if !errors.Is(err, accounts.ErrBadCredentials) {
			log.Printf("[AUTH] login failed: %v", err)
		}
		c.sendError(loginRefused)
		return
	}
	keys, err := s.users.CurrentKeys(ctx, user.ID)
	if err != nil && !errors.Is(err, accounts.ErrNoKeys) {
		log.Printf("[AUTH] key lookup for %d failed: %v", user.ID, err)
		c.sendError(loginRefused)
		return
	}
	if keys != nil {
		keys.AKRecovery = nil
	}
	c.needKeys = keys == nil
	s.rooms.Bind(c.sess, user.ID, user.Username)
	s.watchCredential(c, user.ID, user.Credential())
	s.conns.promote(c.slot)
	log.Printf("[AUTH] user %d (%q) logged in", user.ID, user.Username)
	c.send(protocol.Control{
		Type: "ACK", Name: user.Username, UserID: user.ID,
		AuthScheme: &user.AuthScheme, AccountKeys: keys,
		DeviceToken: s.issueDeviceToken(user.ID, user.Credential(), time.Now()),
	})
}

// handlePrelogin answers PRELOGIN with how to log in. An unknown identifier
// gets a stable made-up salt.
func (s *Server) handlePrelogin(c *client, msg protocol.Control) {
	if !s.authByIP.allow(c.ip, time.Now()) {
		c.sendError(tooManyAttempts)
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	secret, err := s.users.FakeSaltSecret(ctx)
	if err != nil {
		log.Printf("[AUTH] prelogin failed: %v", err)
		c.sendError("prelogin failed")
		return
	}
	fake := fakeSalt(secret, msg.Identifier) // computed for known accounts too
	scheme, salt, err := s.users.Prelogin(ctx, msg.Identifier)
	if errors.Is(err, accounts.ErrNotFound) {
		scheme, salt, err = 1, fake, nil
	}
	if err != nil {
		log.Printf("[AUTH] prelogin failed: %v", err)
		c.sendError("prelogin failed")
		return
	}
	c.send(protocol.Control{Type: "ACK", AuthScheme: &scheme, Salt: salt})
}

func fakeSalt(secret []byte, identifier string) []byte {
	mac := hmac.New(sha256.New, secret)
	mac.Write(e2ee.FakeSaltInput(strings.ToLower(strings.TrimSpace(identifier))))
	return mac.Sum(nil)[:e2ee.SaltLen]
}

// handleSetKeys gives an account without keys its first ones.
func (s *Server) handleSetKeys(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if !c.needKeys {
		c.sendError(accounts.ErrKeysExist.Error())
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	c.credMu.Lock() // the recheck must not see the new credential first
	keys, credential, err := s.users.SetKeys(ctx, c.sess.UserID, c.credential,
		msg.AccountKeys, msg.RecoveryAuthKey, msg.Salt, msg.AuthKey)
	if err == nil {
		c.credential = credential
	}
	c.credMu.Unlock()
	if err != nil {
		switch {
		case errors.Is(err, accounts.ErrBadKeys), errors.Is(err, accounts.ErrKeysExist),
			errors.Is(err, accounts.ErrBadCredentials):
			c.sendError(err.Error())
		default:
			log.Printf("[KEYS] set keys of %d failed: %v", c.sess.UserID, err)
			c.sendError("set keys failed")
		}
		return
	}
	c.needKeys = false
	keys.AKRecovery = nil
	c.send(protocol.Control{Type: "ACK", AccountKeys: keys,
		DeviceToken: s.issueDeviceToken(c.sess.UserID, credential, time.Now())})
}

// recoveryFailed answers every failure of both recovery messages alike.
func recoveryFailed(c *client, err error) {
	if !errors.Is(err, accounts.ErrBadCredentials) {
		log.Printf("[AUTH] recovery failed: %v", err)
	}
	c.sendError(loginRefused)
}

func (s *Server) handleRecoverBegin(c *client, msg protocol.Control) {
	if !s.authByIP.allow(c.ip, time.Now()) {
		c.sendError(tooManyAttempts)
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	userID, keys, err := s.users.RecoverBegin(ctx, msg.Identifier, msg.RecoveryAuthKey)
	if err != nil {
		recoveryFailed(c, err)
		return
	}
	c.send(protocol.Control{Type: "ACK", UserID: userID, AccountKeys: keys})
}

func (s *Server) handleRecoverFinish(c *client, msg protocol.Control) {
	if !s.authByIP.allow(c.ip, time.Now()) {
		c.sendError(tooManyAttempts)
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	userID, err := s.users.RecoverFinish(ctx, msg.Identifier, msg.RecoveryAuthKey, accounts.Recovery{
		Salt: msg.NewSalt, AuthKey: msg.NewAuthKey, RecoveryAuthKey: msg.NewRecoveryAuthKey,
		Keys: msg.AccountKeys,
	})
	if err != nil {
		recoveryFailed(c, err)
		return
	}
	s.endOtherSessions(userID, c.sess)
	c.send(protocol.Control{Type: "ACK"})
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
	ctx, cancel := storeCtx()
	defer cancel()
	var akPassword []byte
	if msg.AccountKeys != nil {
		akPassword = msg.AccountKeys.AKPassword
	}
	c.credMu.Lock()
	credential, err := s.users.ChangePassword(ctx, c.sess.UserID, msg.AuthKey, msg.NewSalt, msg.NewAuthKey, akPassword)
	if err == nil {
		c.credential = credential
	}
	c.credMu.Unlock()
	if err != nil {
		switch {
		case errors.Is(err, accounts.ErrBadCredentials), errors.Is(err, accounts.ErrNotFound),
			errors.Is(err, accounts.ErrBadKeys):
			c.sendError(err.Error())
		default:
			log.Printf("[UPDATE] failed to change password for user %d: %v", c.sess.UserID, err)
			c.sendError("password update failed")
		}
		return
	}

	s.endOtherSessions(c.sess.UserID, c.sess)
	c.send(protocol.Control{Type: "ACK",
		DeviceToken: s.issueDeviceToken(c.sess.UserID, credential, time.Now())})
}

func (s *Server) handleDeleteUser(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	ctx, cancel := storeCtx()
	err := s.users.Delete(ctx, c.sess.UserID, msg.AuthKey, s.loginGate(msg.DeviceToken, c.ip))
	cancel()

	if errors.Is(err, accounts.ErrBadCredentials) {
		c.sendError(err.Error())
		return
	}
	if err != nil {
		log.Printf("[DELETE] failed to delete user %d: %v", c.sess.UserID, err)
		c.sendError("member deletion failed")
		return // a second reply would answer the client's next request
	}
	c.send(protocol.Control{Type: "ACK"})
}
