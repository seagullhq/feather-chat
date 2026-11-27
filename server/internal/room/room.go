// Package room tracks who is talking where: voice rooms keyed by channel,
// the sessions in them, and the UDP forwarding between room-mates.
package room

import (
	"sort"
	"sync"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// Manager holds map[room]map[ssrc]*Session.
type Manager struct {
	mu    sync.RWMutex
	rooms map[string]map[uint32]*Session
	// claimed holds every SSRC taken at HELLO, in a room or not, so two
	// clients can never end up with the same one.
	claimed map[uint32]*Session
	// bound holds the logged-in sessions.
	bound map[*Session]bool
}

// NewManager builds an empty room set.
func NewManager() *Manager {
	return &Manager{rooms: map[string]map[uint32]*Session{}, claimed: map[uint32]*Session{}}
}

// Claim records the session's SSRC and name and creates its media keys. It
// succeeds once per session, and only for a non-zero SSRC nobody else holds.
// Writes hold Manager.mu, the same lock the UDP goroutine reads them under.
func (m *Manager) Claim(s *Session, ssrc uint32, name string) (up, down []byte, ok bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, taken := m.claimed[ssrc]; taken || ssrc == 0 || s.media != nil {
		return nil, nil, false
	}
	s.media, up, down = newMediaKeys()
	m.claimed[ssrc] = s
	if r := m.rooms[s.room]; r != nil {
		delete(r, s.SSRC)
		r[ssrc] = s
	}
	s.SSRC, s.name = ssrc, name
	return up, down, true
}

// Bind sets the logged-in user. Other goroutines read it under Manager.mu.
func (m *Manager) Bind(s *Session, userID int, username string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if s.UserID != userID {
		s.leaves++ // a ring made as another user is void
	}
	s.UserID, s.Username = userID, username
	if m.bound == nil {
		m.bound = map[*Session]bool{}
	}
	m.bound[s] = true
}

// LoggedIn returns every logged-in session of a user, before or after HELLO.
func (m *Manager) LoggedIn(userID int) []*Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var out []*Session
	for s := range m.bound {
		if s.UserID == userID {
			out = append(out, s)
		}
	}
	return out
}

// SessionsOf returns the HELLO'd sessions that belong to the given users.
func (m *Manager) SessionsOf(userIDs map[int]bool) []*Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var out []*Session
	for _, s := range m.claimed {
		if userIDs[s.UserID] {
			out = append(out, s)
		}
	}
	return out
}

// Release frees the session's SSRC when its control connection ends.
func (m *Manager) Release(s *Session) {
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.bound, s)
	if m.claimed[s.SSRC] == s {
		delete(m.claimed, s.SSRC)
	}
}

// Join puts s in room, dropping it from any previous one.
func (m *Manager) Join(s *Session, room string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.joinLocked(s, room)
}

// Ring returns a mark for a call s is about to make: any Leave after it
// (LEAVE, a channel JOIN, a disconnect) makes JoinCall refuse that mark.
func (m *Manager) Ring(s *Session) uint64 {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return s.leaves
}

// JoinCall puts both sides of an answered call in room. It joins neither and
// returns false when the caller has left since the Ring that gave mark, is in
// a room, or is gone.
// Check and join share one lock, so a later Leave of the caller sees the room.
func (m *Manager) JoinCall(caller, callee *Session, room string, mark uint64) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	if caller.leaves != mark || caller.room != "" || m.claimed[caller.SSRC] != caller {
		return false
	}
	m.joinLocked(caller, room)
	m.joinLocked(callee, room)
	return true
}

// joinLocked is Join; the caller must hold m.mu.
func (m *Manager) joinLocked(s *Session, room string) {
	if s.room != "" && s.room != room {
		m.dropLocked(s) // a session belongs to one room at a time
	}
	s.room = room
	if s.Username != "" {
		s.name = s.Username
	}
	r := m.rooms[room]
	if r == nil {
		r = map[uint32]*Session{}
		m.rooms[room] = r
	}
	r[s.SSRC] = s
}

// RoomOf returns the room s is in, "" when in none.
func (m *Manager) RoomOf(s *Session) string {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return s.room
}

// Leave removes the session from its room and returns that room's name, ""
// when it was in none. Reading and clearing share one lock, so the caller
// cleans up exactly the room that was left.
func (m *Manager) Leave(s *Session) string {
	m.mu.Lock()
	defer m.mu.Unlock()
	left := s.room
	m.dropLocked(s)
	s.room = ""
	s.leaves++
	return left
}

// othersLocked lists the room-mates of s. Unlike Peers it keeps sessions that
// are quiet on UDP: this is for TCP notifications, not media.
func (m *Manager) othersLocked(s *Session) []*Session {
	var out []*Session
	for _, p := range m.rooms[s.room] {
		if p != s {
			out = append(out, p)
		}
	}
	return out
}

// Others is othersLocked under the read lock.
func (m *Manager) Others(s *Session) []*Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.othersLocked(s)
}

// Members lists who is in room, optionally without one session.
func (m *Manager) Members(room string, except *Session) []protocol.Member {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var out []protocol.Member
	for _, p := range m.rooms[room] {
		if p != except {
			out = append(out, memberOf(p))
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].SSRC < out[j].SSRC })
	return out
}

func memberOf(s *Session) protocol.Member {
	return protocol.Member{SSRC: s.SSRC, Name: s.name, Muted: s.muted, Deafened: s.deafened, Sharing: s.sharing}
}

// MemberOf snapshots s as a Member.
func (m *Manager) MemberOf(s *Session) protocol.Member {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return memberOf(s)
}

// SetState updates mute/deafen (nil = unchanged) and returns the new state.
func (m *Manager) SetState(s *Session, muted, deafened *bool) protocol.Member {
	m.mu.Lock()
	defer m.mu.Unlock()
	if muted != nil {
		s.muted = *muted
	}
	if deafened != nil {
		s.deafened = *deafened
	}
	return memberOf(s)
}

// dropLocked removes s from its room. The caller must hold m.mu.
func (m *Manager) dropLocked(s *Session) {
	r := m.rooms[s.room]
	if r == nil {
		return
	}
	delete(r, s.SSRC)
	m.dropShareLocked(s, r)
	if len(r) == 0 {
		delete(m.rooms, s.room)
	}
}

// BySSRC resolves the session that claimed ssrc.
func (m *Manager) BySSRC(ssrc uint32) *Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.claimed[ssrc]
}

// Peers returns the non-stale sessions in s's room, excluding s.
func (m *Manager) Peers(s *Session) []*Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	now := time.Now()
	var out []*Session
	for id, p := range m.rooms[s.room] {
		if id != s.SSRC && !p.stale(now) {
			out = append(out, p)
		}
	}
	return out
}
