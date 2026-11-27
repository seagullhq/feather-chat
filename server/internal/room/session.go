package room

import (
	"crypto/tls"
	"encoding/json"
	"net"
	"sync"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// Session is one connected client (TCP control + UDP media).
type Session struct {
	SSRC uint32
	// room is guarded by Manager.mu: accepting a DM call joins the caller from
	// the callee's goroutine. Read it with Manager.RoomOf.
	room string
	// leaves counts every Leave and change of user, guarded by Manager.mu.
	leaves uint64
	// ChannelID is the DB channel behind Room; only the control goroutine touches it.
	ChannelID int
	// ServerID owns ChannelID; kept after leaving so MEMBER_LEFT can still reach the server.
	ServerID int

	// UserID and Username are bound at LOGIN (Manager.Bind); UserID doubles as
	// the logged-in marker (DB ids start at 1).
	UserID   int
	Username string

	tcp        net.Conn
	wmu        sync.Mutex // guards tcp writes
	name       string     // display name, guarded by Manager.mu
	lastSeen   time.Time
	media      *mediaKeys   // set once by HELLO, guarded by Manager.mu
	udpAddr    *net.UDPAddr // validated by a PATH echo, guarded by Manager.mu
	pinged     bool         // First ping received? Owned by the UDP goroutine.
	muted      bool         // guarded by Manager.mu
	deafened   bool         // guarded by Manager.mu
	shareState              // screen-share state, guarded by Manager.mu
	lastMu     sync.Mutex
}

const writeTimeout = 5 * time.Second

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
	// Pushes are written from other sessions' goroutines: a peer that stopped
	// reading (half-open link, frozen client) must not stall them forever.
	_ = s.tcp.SetWriteDeadline(time.Now().Add(writeTimeout))
	if err := protocol.WriteFrame(s.tcp, b); err != nil {
		// A TLS stream is unusable after a failed write. Close the socket under
		// it: tls.Conn.Close would try to write again and wait for the peer.
		conn := s.tcp
		if t, ok := conn.(*tls.Conn); ok {
			conn = t.NetConn()
		}
		_ = conn.Close() // the read loop notices and cleans up
		return err
	}
	return nil
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
