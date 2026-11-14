package control

import (
	"errors"
	"log"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// handleListChannelTypes answers LIST_CHANNEL_TYPES with the channel type
// catalogue.
func (s *Server) handleListChannelTypes(c *client, _ protocol.Control) {
	if !c.requireAuth() {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	list, err := s.servers.ListChannelType(ctx)
	if err != nil {
		log.Printf("[LIST] failed to list channel types: %v", err)
		c.sendError("channel type listing failed")
		return
	}
	summaries := make([]protocol.ChannelTypeSummary, 0, len(list))
	for _, ctype := range list {
		summaries = append(summaries, protocol.ChannelTypeSummary{
			ID:       ctype.ID,
			TypeName: ctype.TypeName,
		})
	}
	c.send(protocol.Control{Type: "ACK", ChannelTypeList: summaries})
}

// handleListChannels answers LIST_CHANNELS with the channels of one server
// plus the live voice roster per channel.
func (s *Server) handleListChannels(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 {
		c.sendError("invalid server")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	list, err := s.servers.ListChannels(ctx, msg.ServerID, c.sess.UserID)
	if err != nil {
		log.Printf("[LIST] failed to list channels for server %d: %v", msg.ServerID, err)
		c.sendError("channel listing failed")
		return
	}
	summaries := make([]protocol.ChannelSummary, 0, len(list))
	for _, channel := range list {
		summaries = append(summaries, protocol.ChannelSummary{
			ID:          channel.ID,
			Name:        channel.Name,
			ChannelType: channel.ChannelType,
			Members:     s.rooms.Members("ch:"+strconv.Itoa(channel.ID), nil),
		})
	}
	c.send(protocol.Control{Type: "ACK", Channels: summaries})
}

// handleListMembers answers LIST_MEMBERS with the people of one server.
func (s *Server) handleListMembers(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 {
		c.sendError("invalid request")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	list, err := s.servers.ListMembers(ctx, msg.ServerID, c.sess.UserID)
	if err != nil {
		log.Printf("[LIST] failed to list members for server %d: %v", msg.ServerID, err)
		c.sendError("member listing failed")
		return
	}
	summaries := make([]protocol.ServerMemberSummary, 0, len(list))
	for _, m := range list {
		summaries = append(summaries, protocol.ServerMemberSummary{ID: m.ID, Username: m.Username, Owner: m.Owner, Admin: m.Admin})
	}
	c.send(protocol.Control{Type: "ACK", ServerMembers: summaries})
}

// handleCreateChannel answers CREATE_CHANNEL by adding a channel to one
// server.
func (s *Server) handleCreateChannel(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	name := strings.TrimSpace(msg.ChannelName)
	if n := utf8.RuneCountInString(name); n < 3 || n > 255 {
		c.sendError("Channel name must be 3-255 characters")
		return
	}
	channelType, err := strconv.Atoi(strings.TrimSpace(msg.ChannelType))
	if err != nil {
		c.sendError("invalid channel type")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	channel, err := s.servers.CreateChannel(ctx, name, channelType, msg.ServerID, c.sess.UserID)
	if err != nil {
		if errors.Is(err, servers.ErrTaken) {
			c.sendError(servers.ErrTaken.Error())
			return
		}
		log.Printf("[CREATION] failed to create channel: %v", err)
		c.sendError("channel creation failed")
		return
	}
	s.notifyServer(msg.ServerID, c.sess, protocol.Control{Type: "CHANNEL_CREATED", ServerID: msg.ServerID,
		ChannelName: channel.Name, ChannelType: strconv.Itoa(channel.ChannelType), ChannelID: channel.ID})
	c.send(protocol.Control{Type: "ACK", ServerID: msg.ServerID, ChannelName: channel.Name, ChannelType: strconv.Itoa(channel.ChannelType), ChannelID: channel.ID})
}
