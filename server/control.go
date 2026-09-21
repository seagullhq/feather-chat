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
func writeFrame(writer io.Writer, payload []byte) error {
	var hdr [4]byte
	binary.BigEndian.PutUint32(hdr[:], uint32(len(payload)))
	_, err := writer.Write(append(hdr[:], payload...))
	return err
}

func readFrame(reader io.Reader) ([]byte, error) {
	var hdr [4]byte
	if _, err := io.ReadFull(reader, hdr[:]); err != nil {
		return nil, err
	}
	n := binary.BigEndian.Uint32(hdr[:])
	if n == 0 || n >= maxControl {
		return nil, io.ErrUnexpectedEOF
	}
	bytes := make([]byte, n)
	_, err := io.ReadFull(reader, bytes)
	return bytes, err
}

// handleControl runs the per-client TCP control session (HELLO/JOIN/LEAVE).
func handleControl(conn net.Conn, roomManager *RoomManager) {
	defer conn.Close()
	session := &Session{tcp: conn, lastSeen: time.Now()}

	for {
		bytes, err := readFrame(conn)
		if err != nil {
			return
		}
		var message Control
		if json.Unmarshal(bytes, &message) != nil {
			continue // tolerate unknown types (§7 interop rule)
		}
		switch message.Type {
		case "HELLO":
			if message.SSRC == 0 || roomManager.Taken(message.SSRC) {
				session.send(Control{Type: "ACK"})
				continue
			}
			session.SSRC, session.name = message.SSRC, message.Name
			session.send(Control{Type: "ACK", SSRC: session.SSRC})
		case "JOIN":
			session.Room = message.Room
			roomManager.Join(session)
			session.send(Control{Type: "ACK", Room: session.Room})
		case "LEAVE":
			defer roomManager.Leave(session)
		}
	}
}

// readUDP parses the 5-byte header, resolves the session by SSRC and
// forwards the datagram verbatim to room peers.
func readUDP(pc *net.UDPConn, roomManager *RoomManager) {
	buf := make([]byte, 1024)
	for {
		n, udpAddr, err := pc.ReadFromUDP(buf)
		if err != nil {
			log.Println(err)
			return
		}
		h, err := DecodeHeader(buf[:n])
		if err != nil || h.Version != 0 {
			continue
		}

		if n <= HeaderLen {
			log.Printf("[ROOM] received a packet with the wrong header! %d invece che %d", n, HeaderLen)
			continue
		}

		session := roomManager.BySSRC(h.SSRC)
		if session == nil {
			continue
		}
		if session.UDPAddr == nil {
			session.UDPAddr = udpAddr
		}
		packet := make([]byte, n)
		roomManager.forward(pc, session, packet[:copy(packet, buf[:n])])
	}
}
