package room

import (
	"errors"
	"log"
	"net"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// ServeUDP reads the media socket forever: it answers PINGs and forwards
// AUDIO and screen-share packets to room peers (§5). This is the one UDP
// goroutine; control goroutines touch the shared session state only under
// Manager.mu.
func (m *Manager) ServeUDP(pc *net.UDPConn) {
	log.Printf("[MEDIA] listening on %s", pc.LocalAddr())
	// Room for any datagram: on Windows a truncated read is an error, which
	// would let one oversized packet stop the media plane.
	buf := make([]byte, 1<<16)
	for {
		n, udpAddr, err := pc.ReadFromUDP(buf)
		if err != nil {
			if !errors.Is(err, net.ErrClosed) {
				log.Printf("[MEDIA] read failed: %v", err)
			}
			return
		}
		m.receive(pc, udpAddr, buf[:n])
	}
}

// receive applies the receiver rules of transport-security.md §2.3, in order.
// Rejected datagrams are dropped silently: anyone can send them.
func (m *Manager) receive(pc *net.UDPConn, from *net.UDPAddr, datagram []byte) {
	n := len(datagram)
	if n < protocol.MinSealedDatagram || n > protocol.MaxVideoDatagram {
		return
	}
	h, err := protocol.DecodeHeader(datagram)
	if err != nil || h.Version != protocol.MediaVersion {
		return
	}
	if h.Kind != protocol.KindAudio && h.Kind != protocol.KindPing && h.Kind != protocol.KindPath && !protocol.IsShareKind(h.Kind) {
		return
	}
	if n > protocol.MaxDatagram && h.Kind != protocol.KindVideo {
		return
	}
	if h.Terminator && h.Kind != protocol.KindAudio && h.Kind != protocol.KindSysAudio {
		return
	}
	s := m.BySSRC(h.SSRC)
	if s == nil {
		return
	}
	keys, _ := m.target(s) // set before s was claimed, so never nil here
	counter, payload, ok := protocol.OpenMedia(keys.up, datagram)
	if !ok {
		return
	}
	newest, ok := keys.replay.Accept(counter)
	if !ok {
		return
	}
	if h.Kind == protocol.KindPath {
		// Not tied to newest: media sent by other threads may overtake the echo.
		s.Touch()
		if keys.path.answers(from, payload, time.Now()) {
			m.setAddr(s, from)
		}
		return
	}
	// Each JOIN opens a new client UDP socket (new port) on the same session,
	// and NATs rebind ports too. The source of the latest datagram may be
	// spoofed, so it only gets a challenge until it echoes it. A reordered
	// datagram must not move the address back.
	_, addr := m.target(s)
	validated := sameAddr(addr, from)
	if newest && !validated {
		m.challenge(pc, keys, h.SSRC, from)
	}
	// An echo from here is what tells the client its address is validated.
	if h.Kind == protocol.KindPing && !validated {
		s.Touch()
		return
	}
	m.forward(pc, s, h, payload)
}

// forward seals payload again for each of session's room peers (sender
// excluded), and answers a PING by echoing it back to session.
func (m *Manager) forward(pc *net.UDPConn, session *Session, header protocol.Header, payload []byte) {
	session.Touch()
	if header.Kind == protocol.KindPing {
		m.sendTo(pc, session, header, payload)
		session.pinged = true
		return
	}

	if !session.pinged {
		return // AUDIO before the first echo: drop, do not forward the packet
	}

	peers := m.Peers(session)
	if protocol.IsShareKind(header.Kind) {
		peers = m.Watchers(session) // share media goes to subscribed viewers only
	}
	for _, p := range peers {
		m.sendTo(pc, p, header, payload)
	}
}

// sendTo seals payload with p's downlink key. Sessions without keys or without
// a learnt address get nothing.
func (m *Manager) sendTo(pc *net.UDPConn, p *Session, header protocol.Header, payload []byte) {
	keys, addr := m.target(p)
	if keys == nil || addr == nil {
		return
	}
	_, _ = pc.WriteToUDP(keys.seal(header, payload), addr) // media is lossy anyway
}

// target reads the media state of s under the lock.
func (m *Manager) target(s *Session) (*mediaKeys, *net.UDPAddr) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return s.media, s.udpAddr
}

func (m *Manager) setAddr(s *Session, addr *net.UDPAddr) {
	m.mu.Lock()
	defer m.mu.Unlock()
	s.udpAddr = addr
}

// displayName returns s's name under the lock, for logging from the UDP
// goroutine (Claim writes it from a control goroutine).
func (m *Manager) displayName(s *Session) string {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return s.name
}
