package room

import (
	"crypto/rand"
	"crypto/subtle"
	"net"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// Return routability (transport-security.md §2.4): media goes to an address
// only after it echoed random bytes the server sent there, so a client that
// spoofs its source address cannot aim a room's media at someone else.

const (
	// pathInterval is the minimum gap between two challenges of one session. It
	// is below the client's 200 ms PING period, so every retry gets through.
	pathInterval = 100 * time.Millisecond
	// pathTTL is how long after the latest challenge its echo still counts.
	pathTTL = 10 * time.Second
)

// pathState is the pending challenge of one session.
type pathState struct {
	addr  *net.UDPAddr // challenged address, nil when none is pending
	bytes [protocol.PathLen]byte
	sent  time.Time
}

// challenge sends a PATH datagram to from, unless one was sent too recently.
// Retries to the same address repeat the bytes, so an echo slower than the
// retry period still matches.
func (m *Manager) challenge(pc *net.UDPConn, keys *mediaKeys, ssrc uint32, from *net.UDPAddr) {
	p, now := &keys.path, time.Now()
	if now.Sub(p.sent) < pathInterval {
		return
	}
	if !sameAddr(p.addr, from) {
		p.addr = from
		rand.Read(p.bytes[:]) // never fails
	}
	p.sent = now
	h := protocol.Header{Kind: protocol.KindPath, SSRC: ssrc}
	_, _ = pc.WriteToUDP(keys.seal(h, p.bytes[:]), from) // lost: the next datagram retries
}

// answers reports whether payload, received from from, echoes the pending
// challenge, and consumes the challenge if so.
func (p *pathState) answers(from *net.UDPAddr, payload []byte, now time.Time) bool {
	if !sameAddr(p.addr, from) || now.Sub(p.sent) > pathTTL || subtle.ConstantTimeCompare(p.bytes[:], payload) != 1 {
		return false
	}
	p.addr = nil
	return true
}

// sameAddr reports whether b is the address a; a nil a is no address.
func sameAddr(a, b *net.UDPAddr) bool {
	return a != nil && a.Port == b.Port && a.Zone == b.Zone && a.IP.Equal(b.IP)
}
