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
}

// NewManager builds an empty room set.
func NewManager() *Manager {
	return &Manager{rooms: map[string]map[uint32]*Session{}, claimed: map[uint32]*Session{}}
}

// Claim records the session's SSRC and name, re-keying its room entry if the
// SSRC changed. It fails if another session already holds the SSRC. Writes
// hold Manager.mu, the same lock the UDP goroutine reads them under.
func (m *Manager) Claim(s *Session, ssrc uint32, name string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	if o, ok := m.claimed[ssrc]; ok && o != s {
		return false
	}
	if s.SSRC != 0 && s.SSRC != ssrc {
		delete(m.claimed, s.SSRC)
	}
	m.claimed[ssrc] = s
	if r := m.rooms[s.Room]; r != nil && s.SSRC != ssrc {
		delete(r, s.SSRC)
		r[ssrc] = s
	}
	s.SSRC, s.name = ssrc, name
	return true
}

// Release frees the session's SSRC when its control connection ends.
func (m *Manager) Release(s *Session) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.claimed[s.SSRC] == s {
		delete(m.claimed, s.SSRC)
	}
}

// Join puts s in room, dropping it from any previous one.
func (m *Manager) Join(s *Session, room string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if s.Room != "" && s.Room != room {
		m.dropLocked(s) // a session belongs to one room at a time
	}
	s.Room = room
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

// Leave removes the session from its room and returns who was left behind,
// so the caller can tell them.
func (m *Manager) Leave(s *Session) []*Session {
	m.mu.Lock()
	defer m.mu.Unlock()
	peers := m.othersLocked(s)
	m.dropLocked(s)
	s.Room = ""
	return peers
}

// othersLocked lists the room-mates of s. Unlike Peers it keeps sessions that
// are quiet on UDP: this is for TCP notifications, not media.
func (m *Manager) othersLocked(s *Session) []*Session {
	var out []*Session
	for _, p := range m.rooms[s.Room] {
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
	return protocol.Member{SSRC: s.SSRC, Name: s.name, Muted: s.muted, Deafened: s.deafened}
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
	r := m.rooms[s.Room]
	if r == nil {
		return
	}
	delete(r, s.SSRC)
	if len(r) == 0 {
		delete(m.rooms, s.Room)
	}
}

// BySSRC resolves the live session that claimed ssrc.
func (m *Manager) BySSRC(ssrc uint32) *Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	now := time.Now()
	for _, r := range m.rooms {
		if s := r[ssrc]; s != nil && !s.stale(now) {
			return s
		}
	}
	return nil
}

// Peers returns the non-stale sessions in s's room, excluding s.
func (m *Manager) Peers(s *Session) []*Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	now := time.Now()
	var out []*Session
	for id, p := range m.rooms[s.Room] {
		if id != s.SSRC && !p.stale(now) {
			out = append(out, p)
		}
	}
	return out
}
