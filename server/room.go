package main

import (
	"net"
	"sync"
)

// RoomManager holds map[room]map[ssrc]*Session.
type RoomManager struct {
	mu    sync.RWMutex
	rooms map[string]map[uint32]*Session
}

func NewRoomManager() *RoomManager {
	return &RoomManager{rooms: map[string]map[uint32]*Session{}}
}

func (rm *RoomManager) Join(s *Session) {
	rm.mu.Lock()
	defer rm.mu.Unlock()
	r := rm.rooms[s.Room]
	if r == nil {
		r = map[uint32]*Session{}
		rm.rooms[s.Room] = r
	}
	r[s.SSRC] = s
}

// Leave removes the session from its room.
func (rm *RoomManager) Leave(s *Session) {
	rm.mu.Lock()
	defer rm.mu.Unlock()
	if r := rm.rooms[s.Room]; r != nil {
		delete(r, s.SSRC)
		if len(r) == 0 {
			delete(rm.rooms, s.Room)
		}
	}
}

// Taken reports whether the SSRC is already held (collision at HELLO).
func (rm *RoomManager) Taken(ssrc uint32) bool {
	rm.mu.RLock()
	defer rm.mu.RUnlock()
	for _, r := range rm.rooms {
		if _, ok := r[ssrc]; ok {
			return true
		}
	}
	return false
}

func (rm *RoomManager) BySSRC(ssrc uint32) *Session {
	rm.mu.RLock()
	defer rm.mu.RUnlock()
	for _, r := range rm.rooms {
		if s := r[ssrc]; s != nil {
			return s
		}
	}
	return nil
}

// forward sends buf verbatim to s's room peers (sender excluded), and
// answers a PING by echoing the same datagram back to the source.
func (rm *RoomManager) forward(pc *net.UDPConn, s *Session, buf []byte) {
	h, err := DecodeHeader(buf)
	if err != nil {
		return
	}
	if h.Kind == KindPing {
		pc.WriteToUDP(buf, s.UDPAddr)
		return
	}
	s.touch()
	for _, p := range rm.Peers(s) {
		pc.WriteToUDP(buf, p.UDPAddr)
	}
}

// Peers returns the sessions in s's room, excluding s.
func (rm *RoomManager) Peers(s *Session) []*Session {
	rm.mu.RLock()
	defer rm.mu.RUnlock()
	var out []*Session
	for id, p := range rm.rooms[s.Room] {
		if id != s.SSRC {
			out = append(out, p)
		}
	}
	return out
}
