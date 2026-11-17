package room

import (
	"errors"
	"log"
	"net"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// ServeUDP reads the media socket forever: it answers PINGs and forwards
// AUDIO packets to room peers (§5). This is the one UDP goroutine; control
// goroutines touch the shared session state only under Manager.mu.
func (m *Manager) ServeUDP(pc *net.UDPConn) {
	log.Printf("[MEDIA] listening on %s", pc.LocalAddr())
	buf := make([]byte, 2048)
	for {
		n, udpAddr, err := pc.ReadFromUDP(buf)
		if err != nil {
			if !errors.Is(err, net.ErrClosed) {
				log.Printf("[MEDIA] read failed: %v", err)
			}
			return
		}

		if n > 1024 {
			log.Printf("[CONTROL] received a datagram greater than 1024 (size: %d), skipping...", n)
			continue
		}

		h, err := protocol.DecodeHeader(buf[:n])
		if err != nil || h.Version != 0 {
			continue
		}

		if n <= protocol.HeaderLen {
			log.Printf("[CONTROL] received a packet with the wrong header! %d instead of %d", n, protocol.HeaderLen)
			continue
		}

		if h.Kind != protocol.KindAudio && h.Kind != protocol.KindPing {
			log.Printf("[CONTROL] received a packet with kind not handled by the server! skipping...")
			continue
		}

		if h.Terminator && h.Kind != protocol.KindAudio {
			continue
		}

		if h.Kind == protocol.KindAudio && h.SSRC == 0 {
			continue
		}

		s := m.BySSRC(h.SSRC)
		if s == nil {
			continue
		}
		// Follow the latest source address: each JOIN opens a new client UDP
		// socket (new port) on the same session, and NATs rebind ports too.
		// ponytail: SSRC is not authenticated, so a spoofer could redirect the
		// stream; a per-session media token in HELLO would close that.
		s.UDPAddr = udpAddr
		packet := make([]byte, n)
		m.forward(pc, s, packet[:copy(packet, buf[:n])])
	}
}

// forward sends buf verbatim to s's room peers (sender excluded), and
// answers a PING by echoing the same datagram back to the source.
func (m *Manager) forward(pc *net.UDPConn, session *Session, buf []byte) {
	header, err := protocol.DecodeHeader(buf)
	if err != nil {
		return
	}
	session.Touch()
	if header.Kind == protocol.KindPing {
		pc.WriteToUDP(buf, session.UDPAddr)
		session.pinged = true
		return
	}

	if !session.pinged {
		log.Printf("[ROOM] received a packet before first ping! from %s", m.displayName(session))
		return // AUDIO prima del primo echo: drop, pacchetto da non inoltrare
	}

	for _, p := range m.Peers(session) {
		pc.WriteToUDP(buf, p.UDPAddr)
	}
}

// displayName returns s's name under the lock, for logging from the UDP
// goroutine (Claim writes it from a control goroutine).
func (m *Manager) displayName(s *Session) string {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return s.name
}
