package main

import (
	"encoding/binary"
	"encoding/json"
	"io"
	"log"
	"net"
	"time"
)

const maxControl = 1 << 20

// writeFrame emits §7 control framing: u32 big-endian length + JSON payload.
func writeFrame(w io.Writer, payload []byte) error {
	var hdr [4]byte
	binary.BigEndian.PutUint32(hdr[:], uint32(len(payload)))
	_, err := w.Write(append(hdr[:], payload...))
	return err
}

func readFrame(r io.Reader) ([]byte, error) {
	var hdr [4]byte
	if _, err := io.ReadFull(r, hdr[:]); err != nil {
		return nil, err
	}
	n := binary.BigEndian.Uint32(hdr[:])
	if n == 0 || n >= maxControl {
		return nil, io.ErrUnexpectedEOF
	}
	b := make([]byte, n)
	_, err := io.ReadFull(r, b)
	return b, err
}

// handleControl runs the per-client TCP control session (HELLO/JOIN/LEAVE).
func handleControl(c net.Conn, rm *RoomManager) {
	defer c.Close()
	s := &Session{tcp: c, lastSeen: time.Now()}

	for {
		b, err := readFrame(c)
		if err != nil {
			return
		}
		var msg Control
		if json.Unmarshal(b, &msg) != nil {
			continue // tolerate unknown types (§7 interop rule)
		}
		switch msg.Type {
		case "HELLO":
			if msg.SSRC == 0 || rm.Taken(msg.SSRC) {
				s.send(Control{Type: "ACK"})
				continue
			}
			s.SSRC, s.name = msg.SSRC, msg.Name
			s.send(Control{Type: "ACK", SSRC: s.SSRC})
		case "JOIN":
			s.Room = msg.Room
			rm.Join(s)
			s.send(Control{Type: "ACK", Room: s.Room})
		case "LEAVE":
			rm.Leave(s)
		}
	}
}

// readUDP parses the 5-byte header, resolves the session by SSRC and
// forwards the datagram verbatim to room peers.
func readUDP(pc *net.UDPConn, rm *RoomManager) {
	buf := make([]byte, 1024)
	for {
		n, addr, err := pc.ReadFromUDP(buf)
		if err != nil {
			log.Println(err)
			return
		}
		h, err := DecodeHeader(buf[:n])
		if err != nil || h.Version != 0 {
			continue
		}
		s := rm.BySSRC(h.SSRC)
		if s == nil {
			continue
		}
		if s.UDPAddr == nil {
			s.UDPAddr = addr
		}
		pkt := make([]byte, n)
		rm.forward(pc, s, pkt[:copy(pkt, buf[:n])])
	}
}
