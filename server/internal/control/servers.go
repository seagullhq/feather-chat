package control

import (
	"errors"
	"log"
	"strings"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/friends"
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
			Owner:      server.OwnerID == c.sess.UserID,
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
	s.notifyServer(server.ID, c.sess, protocol.Control{Type: "SERVER_RENAMED", ServerID: server.ID, Name: server.Name})
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
	// Members must be looked up before the rows are gone.
	peers := s.serverSessions(msg.ServerID, c.sess)
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
	notify(peers, protocol.Control{Type: "SERVER_DELETED", ServerID: msg.ServerID})
	c.send(protocol.Control{Type: "ACK", ServerID: msg.ServerID})
}

func (s *Server) handleServerKick(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 {
		c.sendError("server id required")
		return
	}
	targetUserID := msg.UserID
	if targetUserID == 0 {
		targetUserID = msg.TargetUserID
	}
	if targetUserID == 0 {
		targetUserID = msg.User
	}
	if targetUserID == 0 && msg.Member != nil {
		// no ID in Member struct
	}
	if targetUserID == 0 && len(msg.ServerMembers) > 0 {
		targetUserID = msg.ServerMembers[0].ID
	}
	// Members must be looked up before the rows are gone.
	peers := s.serverSessions(msg.ServerID, c.sess)
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.servers.KickUser(ctx, c.sess.UserID, msg.ServerID, targetUserID); err != nil {
		switch {
		case errors.Is(err, servers.ErrNotFound),
			errors.Is(err, servers.ErrCantDelete),
			errors.Is(err, servers.ErrFailedDelete):
			c.sendError(err.Error())
		default:
			log.Printf("[KICK] failed to kick %d from server %d: %v", targetUserID, msg.ServerID, err)
			c.sendError("user kick failed")
		}
		return
	}
	notify(peers, protocol.Control{Type: "SERVER_MEMBERS_CHANGED", ServerID: msg.ServerID})
	notify(s.rooms.SessionsOf(map[int]bool{targetUserID: true}),
		protocol.Control{Type: "USER_KICKED", ServerID: msg.ServerID})
	c.send(protocol.Control{Type: "ACK"})
}

func (s *Server) handleServerBan(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 {
		c.sendError("server id required")
		return
	}
	targetUserID := msg.UserID
	if targetUserID == 0 {
		targetUserID = msg.TargetUserID
	}
	if targetUserID == 0 {
		targetUserID = msg.User
	}
	if targetUserID == 0 && len(msg.ServerMembers) > 0 {
		targetUserID = msg.ServerMembers[0].ID
	}
	// Members must be looked up before the rows are gone.
	peers := s.serverSessions(msg.ServerID, c.sess)
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.servers.BanUser(ctx, c.sess.UserID, msg.ServerID, targetUserID); err != nil {
		switch {
		case errors.Is(err, servers.ErrNotFound),
			errors.Is(err, servers.ErrCantDelete),
			errors.Is(err, servers.ErrFailedDelete):
			c.sendError(err.Error())
		default:
			log.Printf("[BAN] failed to ban %d from server %d: %v", targetUserID, msg.ServerID, err)
			c.sendError("user kick failed")
		}
		return
	}
	notify(peers, protocol.Control{Type: "SERVER_MEMBERS_CHANGED", ServerID: msg.ServerID})
	notify(s.rooms.SessionsOf(map[int]bool{targetUserID: true}),
		protocol.Control{Type: "USER_BANNED", ServerID: msg.ServerID})
	c.send(protocol.Control{Type: "ACK"})
}

// handleServerUnban answers UNBAN_USER_FROM_SERVER. The unbanned user is not
// told: they simply can join with an invite code again.
func (s *Server) handleServerUnban(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 {
		c.sendError("server id required")
		return
	}
	targetUserID := msg.UserID
	if targetUserID == 0 {
		targetUserID = msg.TargetUserID
	}
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.servers.UnbanUser(ctx, c.sess.UserID, msg.ServerID, targetUserID); err != nil {
		switch {
		case errors.Is(err, servers.ErrNotFound):
			c.sendError("user is not banned")
		case errors.Is(err, servers.ErrForbidden):
			c.sendError("only the owner and admins can unban")
		default:
			log.Printf("[UNBAN] failed to unban %d from server %d: %v", targetUserID, msg.ServerID, err)
			c.sendError("user unban failed")
		}
		return
	}
	s.notifyServer(msg.ServerID, c.sess, protocol.Control{Type: "SERVER_MEMBERS_CHANGED", ServerID: msg.ServerID})
	c.send(protocol.Control{Type: "ACK"})
}

// handleServerMakeAdmin answers MAKE_SERVER_ADMIN by granting the ADMIN role
// to a member of a server owned by the session's user.
func (s *Server) handleServerMakeAdmin(c *client, msg protocol.Control) {
	s.changeServerRole(c, msg, true)
}

// handleServerMakeMember answers MAKE_SERVER_MEMBER by revoking the ADMIN role
// from a member of a server owned by the session's user.
func (s *Server) handleServerMakeMember(c *client, msg protocol.Control) {
	s.changeServerRole(c, msg, false)
}

// handleSendFriendRequest answers SEND_FRIENDSHIP_REQUEST by storing a pending
// request from the session's user to a user named by ID or username.
func (s *Server) handleSendFriendRequest(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()

	addresseeID := msg.TargetUserID
	if addresseeID == 0 {
		addresseeID = msg.UserID
	}
	if addresseeID == 0 {
		username := strings.TrimSpace(msg.Username)
		if username == "" {
			c.sendError("target user or username required")
			return
		}
		user, err := s.users.FindByUsername(ctx, username)
		if err != nil {
			if errors.Is(err, accounts.ErrNotFound) {
				c.sendError(accounts.ErrNotFound.Error())
				return
			}
			log.Printf("[FRIENDS] failed to look up %q: %v", username, err)
			c.sendError("friend request failed")
			return
		}
		addresseeID = user.ID
	}

	request, err := s.friends.SendFriendship(ctx, c.sess.UserID, addresseeID)
	if err != nil {
		switch {
		case errors.Is(err, friends.ErrSelfRequest),
			errors.Is(err, friends.ErrAlreadyExists),
			errors.Is(err, friends.ErrNotFound):
			c.sendError(err.Error())
		default:
			log.Printf("[FRIENDS] failed to send request from %d to %d: %v", c.sess.UserID, addresseeID, err)
			c.sendError("friend request failed")
		}
		return
	}
	c.send(protocol.Control{Type: "ACK", TargetUserID: request.AddresseeID})
}

// handleFetchFriendRequest answers FETCH_FRIENDSHIP_REQUEST with the pending
// requests of the session's user.
func (s *Server) handleFetchFriendRequest(c *client, msg protocol.Control) {
}

// changeServerRole applies a promote/demote request from the server owner.
func (s *Server) changeServerRole(c *client, msg protocol.Control, admin bool) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 {
		c.sendError("server id required")
		return
	}
	targetUserID := msg.UserID
	if targetUserID == 0 {
		targetUserID = msg.TargetUserID
	}
	if targetUserID == 0 {
		targetUserID = msg.User
	}
	ctx, cancel := storeCtx()
	defer cancel()
	var err error
	if admin {
		err = s.servers.MakeAdmin(ctx, c.sess.UserID, msg.ServerID, targetUserID)
	} else {
		err = s.servers.MakeMember(ctx, c.sess.UserID, msg.ServerID, targetUserID)
	}
	if err != nil {
		switch {
		case errors.Is(err, servers.ErrNotFound):
			c.sendError("member not found")
		case errors.Is(err, servers.ErrForbidden):
			c.sendError("only the owner can change roles")
		default:
			log.Printf("[ROLE] failed to change role of %d in server %d: %v", targetUserID, msg.ServerID, err)
			c.sendError("role change failed")
		}
		return
	}
	s.notifyServer(msg.ServerID, c.sess, protocol.Control{Type: "SERVER_MEMBERS_CHANGED", ServerID: msg.ServerID})
	c.send(protocol.Control{Type: "ACK"})
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
		if errors.Is(err, servers.ErrBadInvite) || errors.Is(err, servers.ErrBanned) {
			c.sendError(err.Error())
			return
		}
		log.Printf("[JOIN] failed to join server: %v", err)
		c.sendError("join failed")
		return
	}
	s.notifyServer(server.ID, c.sess, protocol.Control{Type: "SERVER_MEMBERS_CHANGED", ServerID: server.ID})
	c.send(protocol.Control{
		Type:       "ACK",
		ServerID:   server.ID,
		Name:       server.Name,
		InviteCode: server.InviteCode,
	})
}
