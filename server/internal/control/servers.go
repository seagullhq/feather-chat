package control

import (
	"errors"
	"log"
	"strings"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// handleListServers answers LIST_SERVERS with the servers the user belongs
// to, newest first.
func (s *Server) handleListServers(c *client, _ protocol.Control) {
	if !c.requireAuth() {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	list, err := s.servers.ListForUser(ctx, c.sess.UserID)
	if err != nil {
		log.Printf("[LIST] failed to list servers: %v", err)
		c.sendError("server listing failed")
		return
	}
	summaries := make([]protocol.ServerSummary, 0, len(list))
	for _, server := range list {
		summaries = append(summaries, protocol.ServerSummary{
			ID:         server.ID,
			Name:       server.Name,
			InviteCode: server.InviteCode,
		})
	}
	c.send(protocol.Control{Type: "ACK", Servers: summaries})
}

// handleCreateServer answers CREATE_SERVER by creating a server owned by
// the session's user.
func (s *Server) handleCreateServer(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	name := strings.TrimSpace(msg.ServerName)
	if n := utf8.RuneCountInString(name); n < 3 || n > 32 {
		c.sendError("server name must be 3-32 characters")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	server, err := s.servers.CreateServer(ctx, c.sess.UserID, name)
	if err != nil {
		if errors.Is(err, servers.ErrTaken) {
			c.sendError(servers.ErrTaken.Error())
			return
		}
		log.Printf("[CREATION] failed to create server: %v", err)
		c.sendError("server creation failed")
		return
	}
	c.send(protocol.Control{
		Type:       "ACK",
		ServerID:   server.ID,
		Name:       server.Name,
		InviteCode: server.InviteCode,
	})
}

// handleUpdateServer answers UPDATE_SERVER by renaming a server owned by
// the session's user.
func (s *Server) handleUpdateServer(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	name := strings.TrimSpace(msg.ServerName)
	if n := utf8.RuneCountInString(name); n < 3 || n > 32 {
		c.sendError("server name must be 3-32 characters")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	server, err := s.servers.UpdateServer(ctx, c.sess.UserID, msg.ServerID, name)
	if err != nil {
		switch {
		case errors.Is(err, servers.ErrTaken),
			errors.Is(err, servers.ErrNotFound),
			errors.Is(err, servers.ErrForbidden):
			c.sendError(err.Error())
		default:
			log.Printf("[UPDATE] failed to update server %d: %v", msg.ServerID, err)
			c.sendError("server update failed")
		}
		return
	}
	c.send(protocol.Control{
		Type:       "ACK",
		ServerID:   server.ID,
		Name:       server.Name,
		InviteCode: server.InviteCode,
	})
}

// handleDeleteServer answers DELETE_SERVER by dropping a server owned by
// the session's user.
func (s *Server) handleDeleteServer(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 {
		c.sendError("server id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.servers.DeleteServer(ctx, c.sess.UserID, msg.ServerID); err != nil {
		switch {
		case errors.Is(err, servers.ErrNotFound),
			errors.Is(err, servers.ErrCantDelete),
			errors.Is(err, servers.ErrFailedDelete):
			c.sendError(err.Error())
		default:
			log.Printf("[DELETION] failed to delete server %d: %v", msg.ServerID, err)
			c.sendError("server deletion failed")
		}
		return
	}
	c.send(protocol.Control{Type: "ACK", ServerID: msg.ServerID})
}

// handleJoinServer answers JOIN_SERVER by adding the user to the server
// that owns the invite code.
func (s *Server) handleJoinServer(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	code := strings.TrimSpace(msg.InviteCode)
	if code == "" || len(code) > 32 {
		c.sendError(servers.ErrBadInvite.Error())
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	server, err := s.servers.JoinServer(ctx, c.sess.UserID, code)
	if err != nil {
		if errors.Is(err, servers.ErrBadInvite) {
			c.sendError(err.Error())
			return
		}
		log.Printf("[JOIN] failed to join server: %v", err)
		c.sendError("join failed")
		return
	}
	c.send(protocol.Control{
		Type:       "ACK",
		ServerID:   server.ID,
		Name:       server.Name,
		InviteCode: server.InviteCode,
	})
}
