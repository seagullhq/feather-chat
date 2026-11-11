package control

import (
	"context"
	"encoding/json"
	"net"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
)

// client is the per-connection state of the control plane: the session it
// drives and the server that drives it.
type client struct {
	srv  *Server
	sess *room.Session
}

// handleConn runs the per-client TCP control session (HELLO/JOIN/LEAVE)
// until the link dies or the listener stops.
func (s *Server) handleConn(conn net.Conn) {
	defer conn.Close()
	c := &client{srv: s, sess: room.NewSession(conn)}
	// disconnect drops the media entry and tells the room-mates. It also
	// runs when the TCP link dies, so nobody stays listed as a ghost.
	defer s.disconnect(c)

	for {
		payload, err := protocol.ReadFrame(conn)
		if err != nil {
			return
		}
		var msg protocol.Control
		if json.Unmarshal(payload, &msg) != nil {
			continue // tolerate unknown types (§7 interop rule)
		}
		s.dispatch(c, msg)
	}
}

// disconnect leaves any room and frees the SSRC; the TCP link is dead by
// then, and conn.Close runs right after.
func (s *Server) disconnect(c *client) {
	s.leaveRoom(c)
	s.rooms.Release(c.sess)
}

// notify pushes msg to every session in peers. A dead peer is its own
// goroutine's problem: a failed write is ignored here.
func notify(peers []*room.Session, msg protocol.Control) {
	for _, p := range peers {
		_ = p.Send(msg)
	}
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
