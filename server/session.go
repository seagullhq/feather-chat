package main

import (
	"encoding/json"
	"net"
	"sync"
	"time"
)

// Control is a TCP control message (§7): HELLO, JOIN, LEAVE, ACK, ROSTER.
type Control struct {
	Type string `json:"type"`
	Name string `json:"name,omitempty"`
	SSRC uint32 `json:"ssrc,omitempty"`
	Room string `json:"room,omitempty"`
}

// Session is one connected client (TCP control + UDP media).
type Session struct {
	SSRC    uint32
	Room    string
	UDPAddr *net.UDPAddr

	tcp      net.Conn
	wmu      sync.Mutex
	name     string
	lastSeen time.Time
	pinged   bool // Prima ping ricevuta?
	lastMu   sync.Mutex
}

func (session *Session) stale(now time.Time) bool {
	session.lastMu.Lock()
	defer session.lastMu.Unlock()
	return now.Sub(session.lastSeen) > 60*time.Second
}

func (session *Session) touch() {
	session.lastMu.Lock()
	session.lastSeen = time.Now()
	session.lastMu.Unlock()
}

func (session *Session) send(control Control) error {
	b, err := json.Marshal(control)
	if err != nil {
		return err
	}
	session.wmu.Lock()
	defer session.wmu.Unlock()
	return writeFrame(session.tcp, b)
}
