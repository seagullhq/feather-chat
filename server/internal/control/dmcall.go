package control

import (
	"log"
	"slices"
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
	// from is the session that rang; only it can be joined on accept, and only
	// if it has not left since mark (see room.Manager.Ring).
	from *room.Session
	mark uint64
	// startedAt is when it was answered; zero while ringing.
	startedAt time.Time
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
func (m *callManager) begin(roomName string, caller, callee int, from *room.Session, mark uint64) (*dmCall, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, exists := m.calls[roomName]; exists {
		return nil, false
	}
	call := &dmCall{room: roomName, caller: caller, callee: callee, from: from, mark: mark}
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
	call.startedAt = time.Now()
	return call, true
}

// take removes the pair's active call, reading and removing under one lock: of
// two simultaneous leaves only one gets it. A ringing call stays: nobody is in
// its room yet, so a late leave of an earlier call must not end it.
func (m *callManager) take(roomName string) (call *dmCall, startedAt time.Time) {
	m.mu.Lock()
	defer m.mu.Unlock()
	call = m.calls[roomName]
	if call == nil || !call.active {
		return nil, time.Time{}
	}
	delete(m.calls, roomName)
	return call, call.startedAt
}

// reject drops a ringing call and returns it. Either participant may decline
// it: the callee refuses the ring, and the caller uses the same path when their
// client is already busy locally (the server has no room state for a ring yet).
func (m *callManager) reject(roomName string, user int) (*dmCall, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	call := m.calls[roomName]
	if call == nil || call.active || (call.callee != user && call.caller != user) {
		return nil, false
	}
	delete(m.calls, roomName)
	return call, true
}

// cancel withdraws a ringing call and returns it. Only the caller may cancel it.
func (m *callManager) cancel(roomName string, user int) (*dmCall, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	call := m.calls[roomName]
	if call == nil || call.active || call.caller != user {
		return nil, false
	}
	delete(m.calls, roomName)
	return call, true
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

// notifyDmCallState tells the other side of a DM call how this session is now:
// mute/deafen and, on a share change, the SSRC of the sharing screen.
func (s *Server) notifyDmCallState(c *client, roomName string, me protocol.Member) {
	muted, deafened := me.Muted, me.Deafened
	notify(s.rooms.Others(c.sess), protocol.Control{
		Type: "DM_CALL_STATE", Room: roomName, UserID: c.sess.UserID, SSRC: c.sess.SSRC,
		Muted: &muted, Deafened: &deafened, Sharing: me.Sharing,
	})
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

// recordDMCall logs a finished call in the pair's conversation and pushes it
// to both users (every device), with the other party's id in UserID so each
// client files it under the right chat. The message is stored, so the log
// survives a reload and shows the same on both sides.
func (s *Server) recordDMCall(call *dmCall, outcome string, startedAt time.Time) {
	seconds := 0
	if outcome == "ended" && !startedAt.IsZero() {
		seconds = int(time.Since(startedAt).Round(time.Second) / time.Second)
	}
	ctx, cancel := storeCtx()
	defer cancel()
	stored, err := s.messages.RecordCall(ctx, call.caller, call.callee, outcome, seconds)
	if err != nil {
		log.Printf("[DM CALL] log %s call %d -> %d failed: %v", outcome, call.caller, call.callee, err)
		return
	}
	summary := messageSummary(stored.Wire())
	s.dmNotify(call.caller, nil, protocol.Control{
		Type: "DIRECT_MESSAGE", UserID: call.callee, DirectInfo: &summary,
	})
	s.dmNotify(call.callee, nil, protocol.Control{
		Type: "DIRECT_MESSAGE", UserID: call.caller, DirectInfo: &summary,
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
	if s.rooms.RoomOf(c.sess) != "" {
		c.sendError("busy")
		return
	}
	// A peer already in a voice room (server channel or DM call) cannot ring.
	for _, p := range s.rooms.SessionsOf(map[int]bool{peer: true}) {
		if s.rooms.RoomOf(p) != "" {
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
	mark := s.rooms.Ring(c.sess)
	call, ok := s.calls.begin(roomName, c.sess.UserID, peer, c.sess, mark)
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

// handleDmCallAccept joins both sessions to the pair's room: the callee's and
// the one that rang. A caller that has since left, joined a channel or
// disconnected is not pulled in; the call ends as cancelled.
func (s *Server) handleDmCallAccept(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	peer, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	if c.sess.SSRC == 0 {
		c.sendError("hello required")
		return
	}
	roomName := dmRoom(c.sess.UserID, peer)
	call, ok := s.calls.accept(roomName, c.sess.UserID)
	if !ok {
		c.sendError("no incoming call")
		return
	}
	s.leaveRoom(c) // as JOIN does: the callee's old room hears MEMBER_LEFT
	caller := call.from
	if !s.rooms.JoinCall(caller, c.sess, roomName, call.mark) {
		s.calls.drop(roomName, call) // active, so no one else ends it
		s.dmNotifyBoth(call, c.sess, "cancelled")
		s.recordDMCall(call, "cancelled", time.Time{})
		c.sendError("caller unavailable")
		return
	}
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
	call, ok := s.calls.reject(roomName, c.sess.UserID)
	if !ok {
		c.sendError("no incoming call")
		return
	}
	s.dmNotifyBoth(call, c.sess, "rejected")
	s.recordDMCall(call, "rejected", time.Time{})
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
	call, ok := s.calls.cancel(roomName, c.sess.UserID)
	if !ok {
		c.sendError("no outgoing call")
		return
	}
	s.dmNotifyBoth(call, c.sess, "cancelled")
	s.recordDMCall(call, "cancelled", time.Time{})
	c.send(protocol.Control{Type: "ACK", Room: roomName})
}

// dmCallTimeout drops a call nobody answered. expire() is a no-op if the call
// was accepted or replaced, so a late timer cannot end a live call.
func (s *Server) dmCallTimeout(roomName string, call *dmCall) {
	if !s.calls.expire(roomName, call) {
		return
	}
	s.dmNotifyBoth(call, nil, "timeout")
	s.recordDMCall(call, "missed", time.Time{})
}

// onDmCallLeave ends an active call when a session drops its media room
// (LEAVE, a switch to a server channel, or a dead TCP link).
func (s *Server) onDmCallLeave(c *client, roomName string) {
	call, startedAt := s.calls.take(roomName)
	if call == nil {
		return
	}
	s.dmNotifyBoth(call, c.sess, "left")
	s.recordDMCall(call, "ended", startedAt)
}

// dmCancelPendingFor clears the rings a disconnecting session ends: those it
// made, and those to its user when no other session of theirs can answer.
// Active calls are already handled by leaveRoom, which runs first.
func (s *Server) dmCancelPendingFor(gone *room.Session) {
	userID := gone.UserID
	reachable := slices.ContainsFunc(s.rooms.SessionsOf(map[int]bool{userID: true}),
		func(p *room.Session) bool { return p != gone })
	for _, call := range s.calls.all() {
		// active is not read here: accept writes it, and expire checks it
		// under the lock.
		if call.from != gone && (call.callee != userID || reachable) {
			continue
		}
		if !s.calls.expire(call.room, call) {
			continue
		}
		s.dmNotifyBoth(call, nil, "offline")
		s.recordDMCall(call, "offline", time.Time{})
	}
}
