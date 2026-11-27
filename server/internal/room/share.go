package room

import (
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// shareState is the screen-share part of a Session. Every field is guarded
// by Manager.mu; only the methods in this file touch it.
type shareState struct {
	sharing bool
	// watchers are the room-mates subscribed to this session's share.
	watchers map[*Session]struct{}
}

// dropShareLocked ends s's share and unsubscribes s from the shares of the
// room-mates in r. It runs when s leaves r; the caller must hold m.mu.
func (m *Manager) dropShareLocked(s *Session, r map[uint32]*Session) {
	s.sharing, s.watchers = false, nil
	for _, p := range r {
		delete(p.watchers, s) // delete on a nil map is a no-op
	}
}

// MaxSharers caps simultaneous screen shares per room.
const MaxSharers = 4

// StartShare marks s as sharing. ok is false when s is not in a room or the
// room already holds MaxSharers other sharers.
func (m *Manager) StartShare(s *Session) (protocol.Member, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	r := m.rooms[s.room]
	if r == nil {
		return protocol.Member{}, false
	}
	n := 0
	for _, p := range r {
		if p.sharing && p != s {
			n++
		}
	}
	if n >= MaxSharers {
		return protocol.Member{}, false
	}
	s.sharing = true
	return memberOf(s), true
}

// StopShare clears s's share and its watchers.
func (m *Manager) StopShare(s *Session) protocol.Member {
	m.mu.Lock()
	defer m.mu.Unlock()
	s.sharing, s.watchers = false, nil
	return memberOf(s)
}

// Watch subscribes viewer to the share of the room-mate with ssrc and
// returns it; nil when there is no such sharing room-mate.
func (m *Manager) Watch(viewer *Session, ssrc uint32) *Session {
	m.mu.Lock()
	defer m.mu.Unlock()
	t := m.rooms[viewer.room][ssrc]
	if t == nil || t == viewer || !t.sharing {
		return nil
	}
	if t.watchers == nil {
		t.watchers = map[*Session]struct{}{}
	}
	t.watchers[viewer] = struct{}{}
	return t
}

// Unwatch drops viewer from the share of the room-mate with ssrc.
func (m *Manager) Unwatch(viewer *Session, ssrc uint32) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if t := m.rooms[viewer.room][ssrc]; t != nil {
		delete(t.watchers, viewer)
	}
}

// Watchers returns the live, UDP-reachable watchers of s; nil unless s is sharing.
func (m *Manager) Watchers(s *Session) []*Session {
	m.mu.RLock()
	defer m.mu.RUnlock()
	if !s.sharing {
		return nil
	}
	now := time.Now()
	var out []*Session
	for p := range s.watchers {
		if p.udpAddr != nil && !p.stale(now) {
			out = append(out, p)
		}
	}
	return out
}
