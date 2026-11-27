package control

import (
	"errors"
	"log"
	"strconv"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// isE2EEError reports whether err is a refusal the client is told verbatim.
func isE2EEError(err error) bool {
	for _, e := range []error{messages.ErrStaleEpoch, messages.ErrEpochConflict, messages.ErrBadShare,
		messages.ErrBadMessage, messages.ErrDuplicateID, messages.ErrBadRevision} {
		if errors.Is(err, e) {
			return true
		}
	}
	return false
}

// allowKeyRequest checks the login and spends the user's key request budget. It
// answers errors itself.
func (s *Server) allowKeyRequest(c *client) bool {
	if !c.requireAuth() {
		return false
	}
	if !s.keyRequests.allow(strconv.Itoa(c.sess.UserID), time.Now()) {
		c.sendError(tooManyAttempts)
		return false
	}
	return true
}

// conversationAddress reads the address of a key request: directRecipientID, or
// serverId and channelId. A blocked DM is refused. It answers errors itself.
func (s *Server) conversationAddress(c *client, msg protocol.Control) (messages.Address, bool) {
	var none messages.Address
	if !s.allowKeyRequest(c) {
		return none, false
	}
	if msg.DirectRecipientID == "" {
		if msg.ServerID <= 0 || msg.ChannelID <= 0 {
			c.sendError("invalid server channel")
			return none, false
		}
		return messages.Address{ServerID: msg.ServerID, Channel: messages.ChannelID(msg.ChannelID)}, true
	}
	peer, err := directRecipientID(msg)
	if err != nil {
		c.sendError(err.Error())
		return none, false
	}
	ctx, cancel := storeCtx()
	defer cancel()
	blocked, err := s.friends.BlockedEitherWay(ctx, c.sess.UserID, peer)
	if err != nil {
		log.Printf("[KEYS] block check %d -> %d failed: %v", c.sess.UserID, peer, err)
		c.sendError("conversation key request failed")
		return none, false
	}
	if blocked {
		c.sendError(friends.ErrBlocked.Error())
		return none, false
	}
	return messages.Address{PeerID: peer}, true
}

func sendKeyError(c *client, op string, err error) {
	switch {
	case isE2EEError(err),
		errors.Is(err, messages.ErrSelfRequest),
		errors.Is(err, messages.ErrNotFound),
		errors.Is(err, messages.ErrServerChannelNotFound):
		c.sendError(err.Error())
	default:
		log.Printf("[KEYS] %s by %d failed: %v", op, c.sess.UserID, err)
		c.sendError("conversation key request failed")
	}
}

func (s *Server) handleGetConversationKeys(c *client, msg protocol.Control) {
	a, ok := s.conversationAddress(c, msg)
	if !ok {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	keys, err := s.messages.ConversationKeys(ctx, c.sess.UserID, a, msg.BeforeEpoch)
	if err != nil {
		sendKeyError(c, "get", err)
		return
	}
	_ = c.sess.SendJSON(struct {
		Type string `json:"type"`
		*messages.ConversationKeys
	}{"ACK", keys})
}

func (s *Server) handleCreateEpoch(c *client, msg protocol.Control) {
	a, ok := s.conversationAddress(c, msg)
	if !ok {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	conv, err := s.messages.CreateEpoch(ctx, c.sess.UserID, a, messages.NewEpoch{
		Epoch: msg.Epoch, KeyCommit: msg.KeyCommit, CreatorKeyID: msg.CreatorKeyID,
		EpochSig: msg.EpochSig, Wraps: msg.Wraps,
	})
	if err != nil {
		sendKeyError(c, "create epoch", err)
		return
	}
	c.send(protocol.Control{Type: "ACK", ConversationID: conv, Epoch: msg.Epoch})
}

func (s *Server) handleShareKeys(c *client, msg protocol.Control) {
	a, ok := s.conversationAddress(c, msg)
	if !ok {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	conv, recipients, err := s.messages.ShareKeys(ctx, c.sess.UserID, a, msg.SenderKeyID, msg.Shares)
	if err != nil {
		sendKeyError(c, "share", err)
		return
	}
	for _, id := range recipients {
		push := protocol.Control{Type: "KEYS_CHANGED", ConversationID: conv,
			ServerID: a.ServerID, ChannelID: int(a.Channel)}
		if a.PeerID != 0 {
			// The peer from the recipient's view.
			peer := c.sess.UserID
			if id == c.sess.UserID {
				peer = a.PeerID
			}
			push.UserID = peer
		}
		for _, t := range s.rooms.SessionsOf(map[int]bool{id: true}) {
			if t != c.sess {
				_ = t.Send(push)
			}
		}
	}
	c.send(protocol.Control{Type: "ACK", ConversationID: conv})
}

func (s *Server) handleRejectWrap(c *client, msg protocol.Control) {
	a, ok := s.conversationAddress(c, msg)
	if !ok {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	conv, err := s.messages.RejectWrap(ctx, c.sess.UserID, a, msg.Epoch, msg.SenderID)
	if err != nil {
		sendKeyError(c, "reject wrap", err)
		return
	}
	c.send(protocol.Control{Type: "ACK", ConversationID: conv})
}

func (s *Server) handleGetUserKeys(c *client, msg protocol.Control) {
	if !s.allowKeyRequest(c) {
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	keys, err := s.users.UserKeys(ctx, c.sess.UserID, msg.UserID, msg.KeyID)
	if err != nil {
		if !errors.Is(err, accounts.ErrNoKeys) {
			log.Printf("[KEYS] user keys of %d for %d failed: %v", msg.UserID, c.sess.UserID, err)
		}
		c.sendError(accounts.ErrNoKeys.Error())
		return
	}
	c.send(protocol.Control{Type: "ACK", UserKeys: keys})
}
