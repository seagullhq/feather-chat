package main

import (
	"log"
	"net"
	"sync"
	"time"
)

// RoomManager holds map[room]map[ssrc]*Session.
type RoomManager struct {
	mu    sync.RWMutex
	rooms map[string]map[uint32]*Session
}

func NewRoomManager() *RoomManager {
	return &RoomManager{rooms: map[string]map[uint32]*Session{}}
}

// Claim records the session's SSRC and name, re-keying its room entry if the
// SSRC changed. Writes hold roomManager.mu, the same lock the UDP goroutine
// reads them under in Peers/BySSRC/forward.
func (roomManager *RoomManager) Claim(s *Session, ssrc uint32, name string) {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	if r := roomManager.rooms[s.Room]; r != nil && s.SSRC != ssrc {
		delete(r, s.SSRC)
		r[ssrc] = s
	}
	s.SSRC, s.name = ssrc, name
}

func (roomManager *RoomManager) Join(s *Session, room string) {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	if s.Room != "" && s.Room != room {
		roomManager.dropLocked(s) // a session belongs to one room at a time
	}
	s.Room = room
	r := roomManager.rooms[room]
	if r == nil {
		r = map[uint32]*Session{}
		roomManager.rooms[room] = r
	}
	r[s.SSRC] = s
}

// Leave removes the session from its room.
func (roomManager *RoomManager) Leave(s *Session) {
	roomManager.mu.Lock()
	defer roomManager.mu.Unlock()
	roomManager.dropLocked(s)
	s.Room = ""
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

// Taken reports whether the SSRC is already held (collision at HELLO).
func (roomManager *RoomManager) Taken(ssrc uint32) bool {
	roomManager.mu.RLock()
	defer roomManager.mu.RUnlock()
	now := time.Now()
	for _, r := range roomManager.rooms {
		if s, ok := r[ssrc]; ok && !s.stale(now) {
			return true
		}
	}
	return false
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
