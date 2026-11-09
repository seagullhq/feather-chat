package main

import (
	"log"
	"net"
	"sort"
	"sync"
	"time"
)

// RoomManager holds map[room]map[ssrc]*Session.
type RoomManager struct {
	mu    sync.RWMutex
	rooms map[string]map[uint32]*Session
	// claimed holds every SSRC taken at HELLO, in a room or not, so two
	// clients can never end up with the same one.
	claimed map[uint32]*Session
}

func NewRoomManager() *RoomManager {
	return &RoomManager{rooms: map[string]map[uint32]*Session{}, claimed: map[uint32]*Session{}}
}

// Claim records the session's SSRC and name, re-keying its room entry if the
// SSRC changed. It fails if another session already holds the SSRC. Writes
// hold roomManager.mu, the same lock the UDP goroutine reads them under.
func (roomManager *RoomManager) Claim(s *Session, ssrc uint32, name string) bool {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	if o, ok := roomManager.claimed[ssrc]; ok && o != s {
		return false
	}
	if s.SSRC != 0 && s.SSRC != ssrc {
		delete(roomManager.claimed, s.SSRC)
	}
	roomManager.claimed[ssrc] = s
	if r := roomManager.rooms[s.Room]; r != nil && s.SSRC != ssrc {
		delete(r, s.SSRC)
		r[ssrc] = s
	}
	s.SSRC, s.name = ssrc, name
	return true
}

// Release frees the session's SSRC when its control connection ends.
func (roomManager *RoomManager) Release(s *Session) {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	if roomManager.claimed[s.SSRC] == s {
		delete(roomManager.claimed, s.SSRC)
	}
}

func (roomManager *RoomManager) Join(s *Session, room string) {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	if s.Room != "" && s.Room != room {
		roomManager.dropLocked(s) // a session belongs to one room at a time
	}
	s.Room = room
	if s.Username != "" {
		s.name = s.Username
	}
	r := roomManager.rooms[room]
	if r == nil {
		r = map[uint32]*Session{}
		roomManager.rooms[room] = r
	}
	r[s.SSRC] = s
}

// Leave removes the session from its room and returns who was left behind,
// so the caller can tell them.
func (roomManager *RoomManager) Leave(s *Session) []*Session {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	peers := roomManager.othersLocked(s)
	roomManager.dropLocked(s)
	s.Room = ""
	return peers
}

// othersLocked lists the room-mates of s. Unlike Peers it keeps sessions that
// are quiet on UDP: this is for TCP notifications, not media.
func (roomManager *RoomManager) othersLocked(s *Session) []*Session {
	var out []*Session
	for _, p := range roomManager.rooms[s.Room] {
		if p != s {
			out = append(out, p)
		}
	}
	return out
}

// Others is othersLocked under the read lock.
func (roomManager *RoomManager) Others(s *Session) []*Session {
	roomManager.mu.RLock()
	defer roomManager.mu.RUnlock()
	return roomManager.othersLocked(s)
}

// Members lists who is in room, optionally without one session.
func (roomManager *RoomManager) Members(room string, except *Session) []Member {
	roomManager.mu.RLock()
	defer roomManager.mu.RUnlock()
	var out []Member
	for _, p := range roomManager.rooms[room] {
		if p != except {
			out = append(out, memberOf(p))
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].SSRC < out[j].SSRC })
	return out
}

func memberOf(s *Session) Member {
	return Member{SSRC: s.SSRC, Name: s.name, Muted: s.muted, Deafened: s.deafened}
}

// MemberOf snapshots s as a Member.
func (roomManager *RoomManager) MemberOf(s *Session) Member {
	roomManager.mu.RLock()
	defer roomManager.mu.RUnlock()
	return memberOf(s)
}

// SetState updates mute/deafen (nil = unchanged) and returns the new state.
func (roomManager *RoomManager) SetState(s *Session, muted, deafened *bool) Member {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	if muted != nil {
		s.muted = *muted
	}
	if deafened != nil {
		s.deafened = *deafened
	}
	return memberOf(s)
}

// dropLocked removes s from its room. The caller must hold roomManager.mu.
func (roomManager *RoomManager) dropLocked(s *Session) {
	r := roomManager.rooms[s.Room]
	if r == nil {
		return
	}
	delete(r, s.SSRC)
	if len(r) == 0 {
		delete(roomManager.rooms, s.Room)
	}
}

func (roomManager *RoomManager) BySSRC(ssrc uint32) *Session {
	roomManager.mu.RLock()
	defer roomManager.mu.RUnlock()
	now := time.Now()
	for _, r := range roomManager.rooms {
		if s := r[ssrc]; s != nil && !s.stale(now) {
			return s
		}
	}
	return nil
}

// forward sends buf verbatim to s's room peers (sender excluded), and
// answers a PING by echoing the same datagram back to the source.
func (roomManager *RoomManager) forward(pc *net.UDPConn, session *Session, buf []byte) {
	header, err := DecodeHeader(buf)
	if err != nil {
		return
	}
	session.touch()
	if header.Kind == KindPing {
		pc.WriteToUDP(buf, session.UDPAddr)
		session.pinged = true
		return
	}

	if !session.pinged {
		log.Printf("[ROOM] received a packet before first ping! from %s", roomManager.displayName(session))
		return // AUDIO prima del primo echo: drop, pacchetto da non inoltrare
	}

	for _, p := range roomManager.Peers(session) {
		pc.WriteToUDP(buf, p.UDPAddr)
	}
}

// displayName returns s's name under the lock, for logging from the UDP
// goroutine (Claim writes it from a control goroutine).
func (roomManager *RoomManager) displayName(s *Session) string {
	roomManager.mu.RLock()
	defer roomManager.mu.RUnlock()
	return s.name
}

// Peers returns the sessions in s's room, excluding s.
func (roomManager *RoomManager) Peers(session *Session) []*Session {
	roomManager.mu.RLock()
	defer roomManager.mu.RUnlock()
	now := time.Now()
	var out []*Session
	for id, p := range roomManager.rooms[session.Room] {
		if id != session.SSRC && !p.stale(now) {
			out = append(out, p)
		}
	}
	return out
}
