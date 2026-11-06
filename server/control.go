package main

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/servers"
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
func handleControl(conn net.Conn, roomManager *RoomManager, users *accounts.Store, serverStore *servers.ServerStore) {
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
		case "REGISTER":
			if err := validateRegistration(message); err != nil {
				session.send(Control{Type: "ERROR", Error: err.Error()})
				continue
			}
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			err := users.Register(ctx, message.Username, message.Email, message.Password)
			cancel()
			if errors.Is(err, accounts.ErrTaken) {
				session.send(Control{Type: "ERROR", Error: accounts.ErrTaken.Error()})
				continue
			}
			if err != nil {
				log.Printf("[AUTH] register failed: %v", err) // never log the password
				session.send(Control{Type: "ERROR", Error: "registration failed"})
				continue
			}
			session.send(Control{Type: "ACK"})
		case "CREATE_SERVER":
			if !session.authed() {
				session.send(Control{Type: "ERROR", Error: "authentication required"})
				continue
			}
			// Converta l'ID utente salvato in sessione (stringa) nel formato intero di PostgreSQL
			ownerID, err := strconv.Atoi(session.UserID)
			if err != nil {
				session.send(Control{Type: "ERROR", Error: "authentication required"})
				continue
			}
			name := strings.TrimSpace(message.ServerName)
			if n := utf8.RuneCountInString(name); n < 3 || n > 32 {
				session.send(Control{Type: "ERROR", Error: "server name must be 3-32 characters"})
				continue
			}
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			server, err := serverStore.CreateServer(ctx, ownerID, name)
			cancel()
			if err != nil {
				if errors.Is(err, servers.ErrTaken) {
					session.send(Control{Type: "ERROR", Error: servers.ErrTaken.Error()})
					continue
				}
				log.Printf("[CREATION] failed to create server: %v", err)
				session.send(Control{Type: "ERROR", Error: "server creation failed"})
				continue
			}
			session.send(Control{Type: "ACK", InviteCode: server.InviteCode})
		case "LOGIN":
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			user, err := users.Authenticate(ctx, message.Identifier, message.Password)
			cancel()
			if err != nil {
				if !errors.Is(err, accounts.ErrBadCredentials) {
					log.Printf("[AUTH] login failed: %v", err)
				}
				session.send(Control{Type: "ERROR", Error: accounts.ErrBadCredentials.Error()})
				continue
			}
			// Salva l'ID utente numerico convertendolo in stringa
			session.UserID = strconv.Itoa(user.ID)
			session.Username = user.Username
			session.send(Control{Type: "ACK", Name: user.Username})

		case "HELLO":
			if message.SSRC == 0 || roomManager.Taken(message.SSRC) {
				session.send(Control{Type: "ACK"})
				continue
			}
			roomManager.Claim(session, message.SSRC, message.Name)
			session.send(Control{Type: "ACK", SSRC: message.SSRC})
		case "JOIN":
			if !session.authed() {
				session.send(Control{Type: "ERROR", Error: "authentication required"})
				continue
			}
			roomManager.Join(session, message.Room)
			session.send(Control{Type: "ACK", Room: message.Room})
		case "LEAVE":
			roomManager.Leave(session)
		}
	}
}

var emailRe = regexp.MustCompile(`^[^@\s]+@[^@\s]+\.[^@\s]+$`)

func validateRegistration(m Control) error {
	if n := utf8.RuneCountInString(strings.TrimSpace(m.Username)); n < 3 || n > 32 {
		return errors.New("username must be 3-32 characters")
	}
	if n := len([]byte(m.Password)); n < 8 || n > 72 {
		return errors.New("password must be 8-72 bytes")
	}
	if !emailRe.MatchString(strings.TrimSpace(m.Email)) {
		return errors.New("invalid email")
	}
	return nil
}

func readUDP(pc *net.UDPConn, roomManager *RoomManager) {
	buf := make([]byte, 2048)
	for {
		n, udpAddr, err := pc.ReadFromUDP(buf)
		if err != nil {
			log.Println(err)
			return
		}

		if n > 1024 {
			log.Printf("[CONTROL] received a datagram greater than 1024 (size: %d), skipping...", n)
			continue
		}

		h, err := DecodeHeader(buf[:n])
		if err != nil || h.Version != 0 {
			continue
		}

		if n <= HeaderLen {
			log.Printf("[CONTROL] received a packet with the wrong header! %d instead of %d", n, HeaderLen)
			continue
		}

		if h.Kind != KindAudio && h.Kind != KindPing {
			log.Printf("[CONTROL] received a packet with kind not handled by the server! skipping...")
			continue
		}

		if h.Terminator && h.Kind != KindAudio {
			continue
		}

		if h.Kind == KindAudio && h.SSRC == 0 {
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
