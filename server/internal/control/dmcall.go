package control

import (
	"log"
	"strconv"
	"sync"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
)

// dmCallRing is how long a caller waits for an answer before the server drops
// the invitation and tells both sides it ended.
const dmCallRing = 30 * time.Second

// dmCall is one 1:1 call. It lives in memory only: a call is a live voice
// session, not content. The room name is derived from the ordered user pair,
// so both sides always agree on it.
type dmCall struct {
	room   string
	caller int
	callee int
	active bool
}

// callManager holds at most one call per user pair. Calls are short-lived, so
// a mutex is enough; the media path uses room.Manager separately.
type callManager struct {
	mu    sync.Mutex
	calls map[string]*dmCall
}

func newCallManager() *callManager {
	return &callManager{calls: map[string]*dmCall{}}
}

// dmRoom names the room of a pair the same way conversations do (low/high).
func dmRoom(a, b int) string {
	low, high := min(a, b), max(a, b)
	return "dm:" + strconv.Itoa(low) + ":" + strconv.Itoa(high)
}

// begin reserves the pair for a ringing call. One call per pair at a time.
func (m *callManager) begin(roomName string, caller, callee int) (*dmCall, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, exists := m.calls[roomName]; exists {
		return nil, false
	}
	call := &dmCall{room: roomName, caller: caller, callee: callee}
	m.calls[roomName] = call
	return call, true
}

// accept promotes a ringing call to active, but only the callee may do it and
// only before the timer dropped it.
func (m *callManager) accept(roomName string, user int) (*dmCall, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	call := m.calls[roomName]
	if call == nil || call.active || call.callee != user {
		return nil, false
	}
	call.active = true
	return call, true
}

// reject drops a ringing call. Either participant may decline it: the callee
// refuses the ring, and the caller uses the same path when their client is
// already busy locally (the server has no room state for a ring yet).
func (m *callManager) reject(roomName string, user int) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	call := m.calls[roomName]
	if call == nil || call.active || (call.callee != user && call.caller != user) {
		return false
	}
	delete(m.calls, roomName)
	return true
}

// cancel withdraws a ringing call. Only the caller may cancel it.
func (m *callManager) cancel(roomName string, user int) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	call := m.calls[roomName]
	if call == nil || call.active || call.caller != user {
		return false
	}
	delete(m.calls, roomName)
	return true
}

// call returns the call of a pair in either phase.
func (m *callManager) call(roomName string) *dmCall {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.calls[roomName]
}

// expire drops a ringing call only if it is still the exact instance the
// caller is holding and it was not accepted in the meantime. The identity and
// state checks share one lock, so a timer can never race with an accept (or
// with a newer call reusing the same room name) and end a live call.
func (m *callManager) expire(roomName string, expected *dmCall) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	call := m.calls[roomName]
	if call != expected || call.active {
		return false
	}
	delete(m.calls, roomName)
	return true
}

// drop removes a specific call. Pointer identity keeps a late timer or leave
// from removing a newer call with the same room name.
func (m *callManager) drop(roomName string, call *dmCall) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.calls[roomName] == call {
		delete(m.calls, roomName)
	}
}

// all snapshots the live calls, for disconnect cleanup.
func (m *callManager) all() []*dmCall {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([]*dmCall, 0, len(m.calls))
	for _, call := range m.calls {
		out = append(out, call)
	}
	return out
}

// dmNotify pushes msg to every live session of user except one.
func (s *Server) dmNotify(user int, except *room.Session, msg protocol.Control) {
	for _, p := range s.rooms.SessionsOf(map[int]bool{user: true}) {
		if p != except {
			_ = p.Send(msg)
		}
	}
}

// dmNotifyBoth tells both sides the call ended. Each recipient gets the OTHER
// party's id: the client matches it against the peer of its own call state.
// except keeps the session that caused the event from seeing its own echo.
func (s *Server) dmNotifyBoth(call *dmCall, except *room.Session, reason string) {
	s.dmNotify(call.caller, except, protocol.Control{
		Type: "DM_CALL_ENDED", Room: call.room, UserID: call.callee, CallReason: reason,
	})
	s.dmNotify(call.callee, except, protocol.Control{
		Type: "DM_CALL_ENDED", Room: call.room, UserID: call.caller, CallReason: reason,
	})
}

// handleDmCall starts a ringing 1:1 call. The engine starts only after answer,
// so nobody's microphone opens during the ring.
func (s *Server) handleDmCall(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	peer, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	if peer == c.sess.UserID {
		c.sendError("cannot call yourself")
		return
	}
	if c.sess.SSRC == 0 {
		c.sendError("hello required")
		return
	}
	if c.sess.Room != "" {
		c.sendError("busy")
		return
	}
	// A peer already in a voice room (server channel or DM call) cannot ring.
	for _, p := range s.rooms.SessionsOf(map[int]bool{peer: true}) {
		if p.Room != "" {
			c.sendError("busy")
			return
		}
	}

	ctx, cancel := storeCtx()
	defer cancel()
	areFriends, err := s.friends.AreFriends(ctx, c.sess.UserID, peer)
	if err != nil {
		log.Printf("[DM CALL] friendship check %d -> %d failed: %v", c.sess.UserID, peer, err)
		c.sendError("call failed")
		return
	}
	if !areFriends {
		c.sendError("friendship required")
		return
	}
	blocked, err := s.friends.BlockedEitherWay(ctx, c.sess.UserID, peer)
	if err != nil {
		log.Printf("[DM CALL] block check %d -> %d failed: %v", c.sess.UserID, peer, err)
		c.sendError("call failed")
		return
	}
	if blocked {
		c.sendError(friends.ErrCallBlocked.Error())
		return
	}

	roomName := dmRoom(c.sess.UserID, peer)
	call, ok := s.calls.begin(roomName, c.sess.UserID, peer)
	if !ok {
		c.sendError("busy")
		return
	}
	time.AfterFunc(dmCallRing, func() { s.dmCallTimeout(roomName, call) })
	s.dmNotify(peer, c.sess, protocol.Control{
		Type: "DM_CALL_INVITE", Room: roomName, UserID: c.sess.UserID, Username: c.sess.Username,
	})
	c.send(protocol.Control{Type: "ACK", Room: roomName, UserID: peer})
}

// handleDmCallAccept joins both sessions to the pair's room. The caller's
// session is found by user id: a second device may have sent the invitation.
func (s *Server) handleDmCallAccept(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	peer, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	roomName := dmRoom(c.sess.UserID, peer)
	call, ok := s.calls.accept(roomName, c.sess.UserID)
	if !ok {
		c.sendError("no incoming call")
		return
	}
	var caller *room.Session
	for _, p := range s.rooms.SessionsOf(map[int]bool{call.caller: true}) {
		caller = p
		break
	}
	if caller == nil {
		s.calls.drop(roomName, call)
		c.sendError("caller unavailable")
		return
	}
	s.rooms.Join(caller, roomName)
	s.rooms.Join(c.sess, roomName)
	caller.Touch() // a long ring must not make the caller look stale
	c.sess.Touch()
	s.dmNotify(call.caller, nil, protocol.Control{
		Type: "DM_CALL_ACCEPTED", Room: roomName, UserID: c.sess.UserID,
	})
	c.send(protocol.Control{
		Type: "ACK", Room: roomName, UserID: call.caller,
		Members: s.rooms.Members(roomName, c.sess),
	})
}

// handleDmCallReject refuses a ringing call.
func (s *Server) handleDmCallReject(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	peer, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	roomName := dmRoom(c.sess.UserID, peer)
	call := s.calls.call(roomName)
	if !s.calls.reject(roomName, c.sess.UserID) {
		c.sendError("no incoming call")
		return
	}
	s.dmNotifyBoth(call, c.sess, "rejected")
	c.send(protocol.Control{Type: "ACK", Room: roomName})
}

// handleDmCallCancel withdraws a ringing call from the caller's side.
func (s *Server) handleDmCallCancel(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	peer, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	roomName := dmRoom(c.sess.UserID, peer)
	call := s.calls.call(roomName)
	if !s.calls.cancel(roomName, c.sess.UserID) {
		c.sendError("no outgoing call")
		return
	}
	s.dmNotifyBoth(call, c.sess, "cancelled")
	c.send(protocol.Control{Type: "ACK", Room: roomName})
}

// dmCallTimeout drops a call nobody answered. expire() is a no-op if the call
// was accepted or replaced, so a late timer cannot end a live call.
func (s *Server) dmCallTimeout(roomName string, call *dmCall) {
	if !s.calls.expire(roomName, call) {
		return
	}
	s.dmNotifyBoth(call, nil, "timeout")
}

// onDmCallLeave ends an active call when a session drops its media room
// (LEAVE, a switch to a server channel, or a dead TCP link).
func (s *Server) onDmCallLeave(c *client, roomName string) {
	call := s.calls.call(roomName)
	if call == nil {
		return
	}
	s.calls.drop(roomName, call)
	s.dmNotifyBoth(call, c.sess, "left")
}

// dmCancelPendingFor clears rings on disconnect. Active calls are already
// handled by leaveRoom, which runs first.
func (s *Server) dmCancelPendingFor(userID int) {
	for _, call := range s.calls.all() {
		if call.active || (call.caller != userID && call.callee != userID) {
			continue
		}
		if !s.calls.expire(call.room, call) {
			continue
		}
		s.dmNotifyBoth(call, nil, "offline")
	}
}
