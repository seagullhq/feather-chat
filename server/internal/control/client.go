package control

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net"
	"sync"
	"syscall"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
)

// client is the per-connection state of the control plane: the session it
// drives and the server that drives it.
type client struct {
	srv  *Server
	sess *room.Session
	// ip is the rate limiting key of the remote address.
	ip string
	// slot is the connection's place in the connection limiter.
	slot *connSlot
	// loginBy is when a connection that has not logged in is dropped.
	loginBy time.Time
	// keyReq throttles SCREEN_KEYREQ per presenter. Owned by this connection's
	// read loop, so it needs no lock.
	keyReq keyReqLimiter
	// needKeys marks a session of an account without keys.
	needKeys bool
	// credential is what the account credUser logged in against: SET_KEYS fails
	// and the session ends if that has changed since. credMu guards these and
	// watching; done stops the recheck.
	credMu     sync.Mutex
	credUser   int
	credential string
	watching   bool
	done       chan struct{}
}

// handleConn runs the per-client TCP control session (HELLO/JOIN/LEAVE)
// until the link dies or the listener stops.
func (s *Server) handleConn(conn *tls.Conn, slot *connSlot) {
	defer conn.Close()
	remote := conn.RemoteAddr().String()
	log.Printf("[CONTROL] connected %s", remote)
	defer log.Printf("[CONTROL] disconnected %s", remote)

	ctx, cancel := context.WithTimeout(context.Background(), handshakeTimeout)
	defer cancel()
	if err := conn.HandshakeContext(ctx); err != nil {
		log.Printf("[CONTROL] TLS handshake with %s failed: %v", remote, err)
		return
	}

	c := &client{srv: s, sess: room.NewSession(conn), ip: slot.ip, slot: slot,
		loginBy: time.Now().Add(preAuthTimeout), done: make(chan struct{})}
	defer close(c.done)
	// disconnect drops the media entry and tells the room-mates. It also
	// runs when the TCP link dies, so nobody stays listed as a ghost.
	defer s.disconnect(c)

	for {
		payload, err := c.readFrame(conn)
		if err != nil {
			// EOF and abrupt client disconnects are normal; only log real errors.
			if !errors.Is(err, io.EOF) && !errors.Is(err, net.ErrClosed) && !errors.Is(err, syscall.ECONNRESET) {
				log.Printf("[CONTROL] read from %s failed: %v", remote, err)
			}
			return
		}
		var msg protocol.Control
		if err := json.Unmarshal(payload, &msg); err != nil {
			log.Printf("[CONTROL] invalid frame from %s: %v", remote, err)
			continue // tolerate unknown types (§7 interop rule)
		}
		log.Printf("[CONTROL] %s type=%.64q user=%d", remote, msg.Type, c.sess.UserID)
		s.dispatch(c, msg)
	}
}

// readFrame reads the next control frame. Until LOGIN succeeds, frames are
// small and the connection has a fixed time to log in: frames do not extend it.
func (c *client) readFrame(conn net.Conn) ([]byte, error) {
	deadline, limit := time.Time{}, 0
	if !c.sess.Authed() {
		deadline, limit = c.loginBy, preAuthMaxFrame
	}
	if err := conn.SetReadDeadline(deadline); err != nil {
		return nil, err
	}
	if limit > 0 {
		return protocol.ReadFrameLimit(conn, limit)
	}
	return protocol.ReadFrame(conn)
}

// disconnect leaves any room and frees the SSRC; the TCP link is dead by
// then, and conn.Close runs right after.
func (s *Server) disconnect(c *client) {
	s.leaveRoom(c)
	s.dmCancelPendingFor(c.sess)
	s.rooms.Release(c.sess)
}

// notify pushes msg to every session in peers. A dead peer is its own
// goroutine's problem: a failed write is ignored here.
func notify(peers []*room.Session, msg protocol.Control) {
	for _, p := range peers {
		_ = p.Send(msg)
	}
}

// serverSessions lists the live sessions of every member of a server, minus
// except. ponytail: one DB query per push, cache per-server members if it hurts.
func (s *Server) serverSessions(serverID int, except *room.Session) []*room.Session {
	ctx, cancel := storeCtx()
	defer cancel()
	ids, err := s.servers.MemberIDs(ctx, serverID)
	if err != nil {
		log.Printf("[PUSH] member lookup for server %d failed: %v", serverID, err)
		return nil
	}
	set := make(map[int]bool, len(ids))
	for _, id := range ids {
		set[id] = true
	}
	var out []*room.Session
	for _, p := range s.rooms.SessionsOf(set) {
		if p != except {
			out = append(out, p)
		}
	}
	return out
}

// notifyServer pushes msg to every member of a server except the sender.
func (s *Server) notifyServer(serverID int, except *room.Session, msg protocol.Control) {
	notify(s.serverSessions(serverID, except), msg)
}

// send delivers one control message, ignoring errors: the read loop notices
// a dead connection on its next frame.
func (c *client) send(msg protocol.Control) {
	_ = c.sess.Send(msg)
}

// sendError answers with an ERROR frame carrying reason.
func (c *client) sendError(reason string) {
	c.send(protocol.Control{Type: "ERROR", Error: reason})
}

// requireAuth reports whether the session is logged in, answering the
// shared "authentication required" error when it is not.
func (c *client) requireAuth() bool {
	if !c.sess.Authed() {
		c.sendError("authentication required")
		return false
	}
	return true
}

// dbTimeout bounds every store call, so a stuck query cannot pin a session
// goroutine forever.
const dbTimeout = 10 * time.Second

// storeCtx returns a fresh context with the store timeout.
func storeCtx() (context.Context, context.CancelFunc) {
	return context.WithTimeout(context.Background(), dbTimeout)
}
