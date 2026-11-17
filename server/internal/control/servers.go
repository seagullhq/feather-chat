package control

import (
	"errors"
	"log"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/messages"
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

// directRecipientID parses the peer ID shared by the DM request types.
func directRecipientID(msg protocol.Control) (int, error) {
	id, err := strconv.Atoi(strings.TrimSpace(msg.DirectRecipientID))
	if err != nil || id <= 0 {
		return 0, errors.New("direct recipient id required")
	}
	return id, nil
}

// directContent applies the same bounds as the conversation_messages check constraint.
func directContent(msg protocol.Control) (string, error) {
	content := msg.DirectContent
	if strings.TrimSpace(content) == "" {
		return "", errors.New("direct message content required")
	}
	if n := utf8.RuneCountInString(content); n > 4000 {
		return "", errors.New("direct message must be at most 4000 characters")
	}
	return content, nil
}

func (s *Server) handleSendDM(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	recipientID, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	content, err := directContent(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}

	ctx, cancel := storeCtx()
	defer cancel()
	blocked, err := s.friends.BlockedEitherWay(ctx, c.sess.UserID, recipientID)
	if err != nil {
		log.Printf("[DM] block check %d -> %d failed: %v", c.sess.UserID, recipientID, err)
		c.sendError("direct message send failed")
		return
	}
	if blocked {
		c.sendError(friends.ErrBlocked.Error())
		return
	}

	dm, err := s.messages.SendDM(ctx, c.sess.UserID, recipientID, msg.DirectReplyTo, content)
	if err != nil {
		switch {
		case errors.Is(err, messages.ErrSelfRequest),
			errors.Is(err, messages.ErrAlreadyExists),
			errors.Is(err, messages.ErrNotFound),
			errors.Is(err, messages.ErrMessageNotFound):
			c.sendError(err.Error())
		default:
			log.Printf("[DM] send from %d to %d failed: %v", c.sess.UserID, recipientID, err)
			c.sendError("direct message send failed")
		}
		return
	}

	log.Printf("[DM] message %d sent from %d to %d", dm.ID, c.sess.UserID, recipientID)
	summary := directMessageSummary(*dm)
	notify(s.rooms.SessionsOf(map[int]bool{recipientID: true}),
		protocol.Control{Type: "DIRECT_MESSAGE", DirectInfo: &summary})
	c.send(protocol.Control{Type: "ACK", DirectInfo: &summary})
}

// handleReceiveDM answers RECIEVE_DIRECT_MESSAGE with the conversation between
// the session's user and the named peer, newest first.
func (s *Server) handleReceiveDM(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	peerID, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	blocked, err := s.friends.BlockedEitherWay(ctx, c.sess.UserID, peerID)
	if err != nil {
		log.Printf("[DM] block check %d -> %d failed: %v", c.sess.UserID, peerID, err)
		c.sendError("direct message history failed")
		return
	}
	if blocked {
		c.sendError(friends.ErrBlocked.Error())
		return
	}

	list, err := s.messages.ListDM(ctx, c.sess.UserID, peerID, msg.DMLimit, msg.DMBeforeID)
	if err != nil {
		switch {
		case errors.Is(err, messages.ErrSelfRequest):
			c.sendError(err.Error())
		default:
			log.Printf("[DM] list for %d with %d failed: %v", c.sess.UserID, peerID, err)
			c.sendError("direct message history failed")
		}
		return
	}

	// Fetching a page means the viewer has seen it; advance the read pointer.
	var newestID int
	if len(list) > 0 {
		newestID = list[0].ID
	}
	state, err := s.messages.MarkDMRead(ctx, c.sess.UserID, peerID, newestID)
	if err != nil {
		log.Printf("[DM] mark read for %d with %d failed: %v", c.sess.UserID, peerID, err)
		c.sendError("direct message history failed")
		return
	}
	if state.Changed {
		notify(s.rooms.SessionsOf(map[int]bool{peerID: true}),
			protocol.Control{Type: "DIRECT_MESSAGE_READ", DirectMessageID: state.LastReadMessageID})
	}
	c.send(protocol.Control{
		Type: "ACK", DirectMessages: directMessageSummaries(list), UnreadCount: state.UnreadCount,
	})
}

func (s *Server) handleUpdateDM(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.DirectMessageID <= 0 {
		c.sendError("direct message id required")
		return
	}
	content, err := directContent(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	edited, peerID, err := s.messages.EditDM(ctx, c.sess.UserID, msg.DirectMessageID, content)
	if err != nil {
		sendDMMutationError(c, "edit", c.sess.UserID, msg.DirectMessageID, err)
		return
	}
	log.Printf("[DM] message %d edited by %d", edited.ID, c.sess.UserID)
	summary := directMessageSummary(*edited)
	notify(s.rooms.SessionsOf(map[int]bool{peerID: true}),
		protocol.Control{Type: "DIRECT_MESSAGE_EDITED", DirectInfo: &summary})
	c.send(protocol.Control{Type: "ACK", DirectInfo: &summary})
}

func (s *Server) handleDeleteDM(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.DirectMessageID <= 0 {
		c.sendError("direct message id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	id, peerID, err := s.messages.DeleteDM(ctx, c.sess.UserID, msg.DirectMessageID)
	if err != nil {
		sendDMMutationError(c, "delete", c.sess.UserID, msg.DirectMessageID, err)
		return
	}
	log.Printf("[DM] message %d deleted by %d", id, c.sess.UserID)
	notify(s.rooms.SessionsOf(map[int]bool{peerID: true}),
		protocol.Control{Type: "DIRECT_MESSAGE_DELETED", DirectMessageID: id})
	c.send(protocol.Control{Type: "ACK", DirectMessageID: id})
}

func (s *Server) handleMarkDMRead(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	peerID, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	state, err := s.messages.MarkDMRead(ctx, c.sess.UserID, peerID, msg.DirectMessageID)
	if err != nil {
		log.Printf("[DM] mark read for %d by %d failed: %v", peerID, c.sess.UserID, err)
		c.sendError("mark direct message read failed")
		return
	}
	if state.Changed {
		notify(s.rooms.SessionsOf(map[int]bool{peerID: true}),
			protocol.Control{Type: "DIRECT_MESSAGE_READ", DirectMessageID: state.LastReadMessageID})
	}
	c.send(protocol.Control{
		Type: "ACK", DirectMessageID: state.LastReadMessageID, UnreadCount: state.UnreadCount,
	})
}

func (s *Server) handleListDMConversations(c *client, _ protocol.Control) {
	if !c.requireAuth() {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	conversations, err := s.messages.ListDMConversations(ctx, c.sess.UserID)
	if err != nil {
		log.Printf("[DM] conversation list for %d failed: %v", c.sess.UserID, err)
		c.sendError("direct conversation list failed")
		return
	}
	out := make([]protocol.DirectConversationSummary, len(conversations))
	for i, conv := range conversations {
		out[i] = protocol.DirectConversationSummary{
			ChannelID: conv.ChannelID, PeerID: conv.PeerID,
			LastMessageID: conv.LastMessageID, Content: conv.Content,
			AuthorID: conv.AuthorID, CreatedAt: conv.CreatedAt, UnreadCount: conv.UnreadCount,
		}
	}
	c.send(protocol.Control{Type: "ACK", DirectConversations: out})
}

func sendDMMutationError(c *client, op string, userID, messageID int, err error) {
	if errors.Is(err, messages.ErrMessageNotFound) {
		c.sendError(err.Error())
		return
	}
	log.Printf("[DM] %s %d by %d failed: %v", op, messageID, userID, err)
	c.sendError("direct message " + op + " failed")
}

func directMessageSummary(m messages.DirectMessage) protocol.DirectMessageSummary {
	return protocol.DirectMessageSummary{
		ID: m.ID, ChannelID: m.ChannelID, AuthorID: m.Author,
		Content: m.Content, ReplyTo: m.ReplyTo,
		CreatedAt: m.CreatedAt, EditedAt: m.EditedAt, DeletedAt: m.DeletedAt,
	}
}

func directMessageSummaries(list []messages.DirectMessage) []protocol.DirectMessageSummary {
	out := make([]protocol.DirectMessageSummary, len(list))
	for i, m := range list {
		out[i] = directMessageSummary(m)
	}
	return out
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

// friendsChanged tells every session of userID that its friend list is stale.
func (s *Server) friendsChanged(userID int) {
	notify(s.rooms.SessionsOf(map[int]bool{userID: true}), protocol.Control{Type: "FRIENDS_CHANGED"})
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
	s.friendsChanged(request.AddresseeID)
	c.send(protocol.Control{Type: "ACK", TargetUserID: request.AddresseeID})
}

// handleSendFriendRequest answers SEND_FRIENDSHIP_REQUEST by storing a pending
// request from the session's user to a user named by ID or username.
func (s *Server) handleRemoveFriend(c *client, msg protocol.Control) {
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

	err := s.friends.RemoveFriendship(ctx, c.sess.UserID, addresseeID)
	if err != nil {
		switch {
		case errors.Is(err, friends.ErrRemoveFriend):
			c.sendError(err.Error())
		default:
			log.Printf("[FRIENDS] failed to remove from %d to %d: %v", c.sess.UserID, addresseeID, err)
			c.sendError("remove friendship failed")
		}
		return
	}
	s.friendsChanged(addresseeID)
	c.send(protocol.Control{Type: "ACK", TargetUserID: addresseeID})
}

// handleFetchFriendRequest answers FETCH_FRIENDSHIP_REQUEST with the pending
// requests of the session's user.
func (s *Server) handleFetchFriendRequest(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	people, err := s.friends.FetchRequests(ctx, c.sess.UserID)
	if err != nil {
		log.Printf("[FRIENDS] failed to fetch for %d: %v", c.sess.UserID, err)
		c.sendError("friend list failed")
		return
	}
	c.send(protocol.Control{Type: "ACK", Friends: personSummaries(people)})
}

// handleAcceptFriendRequest answers ACCEPT_FRIENDSHIP_REQUEST by accepting the
// pending request that user userId sent to the session's user.
func (s *Server) handleAcceptFriendRequest(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.UserID <= 0 {
		c.sendError("requester id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.friends.AcceptFriendship(ctx, c.sess.UserID, msg.UserID); err != nil {
		if errors.Is(err, friends.ErrNotFound) {
			c.sendError("friend request not found")
			return
		}
		log.Printf("[FRIENDS] failed to accept %d for %d: %v", msg.UserID, c.sess.UserID, err)
		c.sendError("accept failed")
		return
	}
	s.friendsChanged(msg.UserID)
	c.send(protocol.Control{Type: "ACK", TargetUserID: msg.UserID})
}

// handleDeclineFriendRequest answers DECLINE_FRIENDSHIP_REQUEST by dropping the
// pending request that user userId sent to the session's user.
func (s *Server) handleDeclineFriendRequest(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.UserID <= 0 {
		c.sendError("requester id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.friends.DeclineFriendship(ctx, c.sess.UserID, msg.UserID); err != nil {
		if errors.Is(err, friends.ErrNotFound) {
			c.sendError("friend request not found")
			return
		}
		log.Printf("[FRIENDS] failed to decline %d for %d: %v", msg.UserID, c.sess.UserID, err)
		c.sendError("decline failed")
		return
	}
	s.friendsChanged(msg.UserID)
	c.send(protocol.Control{Type: "ACK", TargetUserID: msg.UserID})
}

func (s *Server) handleBlockFriend(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.UserID <= 0 {
		c.sendError("requester id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.friends.BlockFriendship(ctx, c.sess.UserID, msg.UserID); err != nil {
		if errors.Is(err, friends.ErrSelfRequest) {
			c.sendError(err.Error())
			return
		}
		log.Printf("[FRIENDS] failed to block %d for %d: %v", msg.UserID, c.sess.UserID, err)
		c.sendError("block failed")
		return
	}
	s.friendsChanged(msg.UserID)
	c.send(protocol.Control{Type: "ACK", TargetUserID: msg.UserID})
}

// handleCancelFriendRequest answers CANCEL_FRIENDSHIP_REQUEST by withdrawing the
// pending request the session's user sent to userId.
func (s *Server) handleCancelFriendRequest(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.UserID <= 0 {
		c.sendError("addressee id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.friends.CancelFriendship(ctx, c.sess.UserID, msg.UserID); err != nil {
		if errors.Is(err, friends.ErrNotFound) {
			c.sendError("friend request not found")
			return
		}
		log.Printf("[FRIENDS] failed to cancel %d for %d: %v", msg.UserID, c.sess.UserID, err)
		c.sendError("cancel failed")
		return
	}
	s.friendsChanged(msg.UserID)
	c.send(protocol.Control{Type: "ACK", TargetUserID: msg.UserID})
}

// handleUnblockFriend answers UNBLOCK_FRIENDSHIP by lifting the block the
// session's user placed on userId. Nothing is restored: they are strangers again.
func (s *Server) handleUnblockFriend(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.UserID <= 0 {
		c.sendError("requester id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	if err := s.friends.UnblockFriendship(ctx, c.sess.UserID, msg.UserID); err != nil {
		if errors.Is(err, friends.ErrNotFound) {
			c.sendError("user is not blocked")
			return
		}
		log.Printf("[FRIENDS] failed to unblock %d for %d: %v", msg.UserID, c.sess.UserID, err)
		c.sendError("unblock failed")
		return
	}
	s.friendsChanged(msg.UserID)
	c.send(protocol.Control{Type: "ACK", TargetUserID: msg.UserID})
}

// handleSearchUsers answers SEARCH_USERS with the users whose name contains
// the query, each with its relation to the session's user.
func (s *Server) handleSearchUsers(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	query := strings.TrimSpace(msg.Query)
	if query == "" {
		c.send(protocol.Control{Type: "ACK"})
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	people, err := s.friends.SearchUsers(ctx, c.sess.UserID, query)
	if err != nil {
		log.Printf("[FRIENDS] search %q failed: %v", query, err)
		c.sendError("user search failed")
		return
	}
	c.send(protocol.Control{Type: "ACK", Users: personSummaries(people)})
}

func personSummaries(people []friends.Person) []protocol.PersonSummary {
	out := make([]protocol.PersonSummary, len(people))
	for i, p := range people {
		out[i] = protocol.PersonSummary{ID: p.ID, Username: p.Username, Relation: p.Relation}
	}
	return out
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
