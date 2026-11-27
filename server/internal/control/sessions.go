package control

import (
	"errors"
	"log"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/room"
)

// credentialRecheck is how often a session compares the credential it logged in
// against with the stored one. With dbTimeout it stays within the 30 s of
// e2ee-text.md §3.4. An operator reset runs in another process, so only this
// notices it.
const credentialRecheck = 20 * time.Second

// watchCredential records what the session logged in against and, on the first
// LOGIN of the connection, starts rechecking it.
func (s *Server) watchCredential(c *client, userID int, credential string) {
	c.credMu.Lock()
	defer c.credMu.Unlock()
	c.credUser, c.credential = userID, credential
	if c.watching {
		return
	}
	c.watching = true
	go func() {
		tick := time.NewTicker(credentialRecheck)
		defer tick.Stop()
		for {
			select {
			case <-c.done:
				return
			case <-tick.C:
				if !s.credentialCurrent(c) {
					log.Printf("[AUTH] ending a session with an outdated credential")
					c.sess.Close()
					return
				}
			}
		}
	}()
}

// credentialCurrent is false only if the stored credential is known to differ. A
// database failure keeps the session until the next check.
func (s *Server) credentialCurrent(c *client) bool {
	c.credMu.Lock()
	defer c.credMu.Unlock()
	ctx, cancel := storeCtx()
	defer cancel()
	stored, err := s.users.StoredCredential(ctx, c.credUser)
	if errors.Is(err, accounts.ErrNotFound) {
		return false
	}
	if err != nil {
		log.Printf("[AUTH] credential recheck of %d failed: %v", c.credUser, err)
		return true
	}
	return stored == c.credential
}

// endOtherSessions drops every logged-in session of userID except keep.
func (s *Server) endOtherSessions(userID int, keep *room.Session) {
	for _, t := range s.rooms.LoggedIn(userID) {
		if t != keep {
			t.Close()
		}
	}
}
