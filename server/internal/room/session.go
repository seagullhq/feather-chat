package room

import (
	"encoding/json"
	"net"
	"sync"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// Session is one connected client (TCP control + UDP media).
type Session struct {
	SSRC uint32
	Room string
	// ChannelID is the DB channel behind Room; only the control goroutine touches it.
	ChannelID int
	// ServerID owns ChannelID; kept after leaving so MEMBER_LEFT can still reach the server.
	ServerID int
	UDPAddr  *net.UDPAddr

	// UserID and Username are bound at LOGIN; UserID doubles as the
	// logged-in marker (DB ids start at 1).
	UserID   int
	Username string

	tcp      net.Conn
	wmu      sync.Mutex // guards tcp writes
	name     string     // display name, guarded by Manager.mu
	lastSeen time.Time
	pinged   bool // Prima ping ricevuta?
	muted    bool // guarded by Manager.mu
	deafened bool // guarded by Manager.mu
	lastMu   sync.Mutex
}

// NewSession starts a session on a freshly accepted control connection.
func NewSession(conn net.Conn) *Session {
	return &Session{tcp: conn, lastSeen: time.Now()}
}

// Authed reports whether the session has logged in.
func (s *Session) Authed() bool { return s.UserID != 0 }

// Send delivers one control message on the TCP link. Concurrent sends are
// serialized; the error is the caller's to notice.
func (s *Session) Send(control protocol.Control) error {
	b, err := json.Marshal(control)
	if err != nil {
		return err
	}
	s.wmu.Lock()
	defer s.wmu.Unlock()
	return protocol.WriteFrame(s.tcp, b)
}

// Touch records that the session was just seen, so it does not look stale.
func (s *Session) Touch() {
	s.lastMu.Lock()
	s.lastSeen = time.Now()
	s.lastMu.Unlock()
}

func (s *Session) stale(now time.Time) bool {
	s.lastMu.Lock()
	defer s.lastMu.Unlock()
	return now.Sub(s.lastSeen) > 60*time.Second
}
