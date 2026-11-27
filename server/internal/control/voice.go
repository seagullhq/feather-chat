package control

import (
	"encoding/base64"
	"errors"
	"log"
	"strconv"
	"strings"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// handleHello answers HELLO by claiming the client's SSRC and handing out the
// session's media keys.
func (s *Server) handleHello(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	up, down, ok := s.rooms.Claim(c.sess, msg.SSRC, msg.Name)
	if !ok {
		// No ssrc in the ACK: it is taken (pick another), or the session
		// already owns one.
		c.send(protocol.Control{Type: "ACK"})
		return
	}
	c.send(protocol.Control{Type: "ACK", SSRC: msg.SSRC,
		MediaKeyUp:   base64.StdEncoding.EncodeToString(up),
		MediaKeyDown: base64.StdEncoding.EncodeToString(down),
	})
}

// handleJoin answers JOIN by moving the session into the channel's room.
func (s *Server) handleJoin(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if c.sess.SSRC == 0 {
		c.sendError("hello required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	channel, err := s.servers.VoiceChannelForUser(ctx, msg.ChannelID, c.sess.UserID)
	if err != nil {
		if errors.Is(err, servers.ErrNotFound) {
			c.sendError(servers.ErrNotFound.Error())
			return
		}
		log.Printf("[JOIN] channel lookup failed: %v", err)
		c.sendError("join failed")
		return
	}
	// One media room per channel row; membership was just checked in the DB.
	roomName := "ch:" + strconv.Itoa(channel.ID)
	s.leaveRoom(c) // a session belongs to one room at a time
	s.rooms.Join(c.sess, roomName)
	c.sess.ChannelID, c.sess.ServerID = channel.ID, channel.ServerID
	c.sess.Touch() // lastSeen starts at TCP connect; a late JOIN must not look stale
	c.send(protocol.Control{Type: "ACK", Room: roomName, ChannelID: channel.ID, ServerID: channel.ServerID,
		Members: s.rooms.Members(roomName, c.sess)})
	me := s.rooms.MemberOf(c.sess)
	s.notifyServer(channel.ServerID, c.sess, protocol.Control{Type: "MEMBER_JOINED", ChannelID: channel.ID, Member: &me})
}

// handleState answers STATE by updating mute/deafen and telling the room.
func (s *Server) handleState(c *client, msg protocol.Control) {
	roomName := s.rooms.RoomOf(c.sess)
	if roomName == "" {
		return
	}
	me := s.rooms.SetState(c.sess, msg.Muted, msg.Deafened)
	if strings.HasPrefix(roomName, "dm:") {
		// DM calls have no server to fan out to: tell the room-mates directly.
		s.notifyDmCallState(c, roomName, me)
		return
	}
	s.notifyServer(c.sess.ServerID, c.sess, protocol.Control{Type: "MEMBER_STATE", ChannelID: c.sess.ChannelID, Member: &me})
}

// handleLeave answers LEAVE by leaving the current room.
func (s *Server) handleLeave(c *client, _ protocol.Control) {
	s.leaveRoom(c)
}

// leaveRoom drops the media entry and tells the room-mates. It also runs
// when the TCP link dies, so nobody stays listed as a ghost.
func (s *Server) leaveRoom(c *client) {
	roomName := s.rooms.Leave(c.sess)
	if roomName == "" {
		return
	}
	if strings.HasPrefix(roomName, "dm:") {
		s.onDmCallLeave(c, roomName)
		return
	}
	s.notifyServer(c.sess.ServerID, c.sess, protocol.Control{Type: "MEMBER_LEFT", ChannelID: c.sess.ChannelID, SSRC: c.sess.SSRC})
}
