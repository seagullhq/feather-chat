package control

import (
	"errors"
	"log"
	"sync"
	"time"

	"golang.org/x/time/rate"

	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// sendServerMessageError uses the same text for unknown IDs, foreign IDs and
// denied access: sequential IDs would otherwise make enumeration trivial.
func sendServerMessageError(c *client, op string, userID int, err error) {
	switch {
	case isE2EEError(err):
		c.sendError(err.Error())
	case errors.Is(err, messages.ErrServerMessageNotFound):
		c.sendError("server message not found")
	case errors.Is(err, messages.ErrServerChannelNotFound):
		c.sendError("server channel not found")
	default:
		log.Printf("[SERVER MSG] %s by %d failed: %v", op, userID, err)
		c.sendError("server message " + op + " failed")
	}
}

type serverMessageLimiter struct {
	send *rate.Limiter
	edit *rate.Limiter
}

var (
	serverMessageLimiters sync.Map // userID -> *serverMessageLimiter
	serverLimiterCleanup  sync.Once
)

// serverMessageLimitersFor keeps one token bucket pair per user. rate.Limiter
// is safe for concurrent use. Idle buckets are reaped every five minutes so
// the map only grows with active senders.
func serverMessageLimitersFor(userID int) *serverMessageLimiter {
	serverLimiterCleanup.Do(func() {
		go func() {
			ticker := time.NewTicker(5 * time.Minute)
			defer ticker.Stop()
			for range ticker.C {
				serverMessageLimiters.Range(func(key, value any) bool {
					l := value.(*serverMessageLimiter)
					if l.send.Tokens() == float64(l.send.Burst()) && l.edit.Tokens() == float64(l.edit.Burst()) {
						serverMessageLimiters.Delete(key)
					}
					return true
				})
			}
		}()
	})
	if v, ok := serverMessageLimiters.Load(userID); ok {
		return v.(*serverMessageLimiter)
	}
	l := &serverMessageLimiter{
		send: rate.NewLimiter(rate.Limit(1), 5),
		edit: rate.NewLimiter(rate.Limit(0.5), 3),
	}
	actual, _ := serverMessageLimiters.LoadOrStore(userID, l)
	return actual.(*serverMessageLimiter)
}

func (s *Server) handleSendServerMessage(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerID <= 0 || msg.ChannelID <= 0 {
		c.sendError("invalid server channel")
		return
	}
	if err := encryptedOnly(msg); err != nil {
		c.sendError(err.Error())
		return
	}
	if !serverMessageLimitersFor(c.sess.UserID).send.Allow() {
		c.sendError("server message rate limit exceeded")
		return
	}

	ctx, cancel := storeCtx()
	defer cancel()
	stored, err := s.messages.SendChannelMessage(ctx, c.sess.UserID, msg.ServerID,
		messages.ChannelID(msg.ChannelID), msg.E2EE, int(msg.ServerReplyTo))
	if err != nil {
		sendServerMessageError(c, "send", c.sess.UserID, err)
		return
	}

	summary := messageSummary(stored.Wire())
	// The serverID comes from the store's view, never from the request: a
	// member of two servers must not push channel B into server A.
	s.notifyServer(stored.ServerID, c.sess, protocol.Control{
		Type: "SERVER_MESSAGE", ServerID: stored.ServerID,
		ChannelID: int(stored.ChannelID), ServerInfo: &summary,
	})
	c.send(protocol.Control{Type: "ACK", ServerID: stored.ServerID,
		ChannelID: int(stored.ChannelID), ServerInfo: &summary})
}

func (s *Server) handleListServerMessages(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ChannelID <= 0 {
		c.sendError("invalid server channel")
		return
	}
	limit := messages.ChannelPageLimit(int(msg.ServerLimit))
	channelID := messages.ChannelID(msg.ChannelID)

	ctx, cancel := storeCtx()
	defer cancel()
	// The store fetches limit+1 and returns the sentinel row; hasMore is
	// decided here, where the requested size is still available.
	list, err := s.messages.ListChannelMessages(ctx, c.sess.UserID, channelID,
		int64(msg.ServerBeforeID), limit)
	if err != nil {
		sendServerMessageError(c, "history", c.sess.UserID, err)
		return
	}

	out, hasMore := pageOf(messageSummaries(list), limit)

	// Only the newest page means "I reached the bottom"; older pages are a
	// keyset cursor and must never move the read pointer backwards.
	if msg.ServerBeforeID == 0 && len(list) > 0 {
		if _, err := s.messages.MarkChannelRead(ctx, c.sess.UserID, channelID, list[0].ID); err != nil {
			log.Printf("[SERVER MSG] mark read %d/%d failed: %v", c.sess.UserID, msg.ChannelID, err)
			c.sendError("server message history failed")
			return
		}
	}
	unread, err := s.messages.UnreadCountForChannel(ctx, c.sess.UserID, channelID)
	if err != nil {
		log.Printf("[SERVER MSG] unread count %d/%d failed: %v", c.sess.UserID, msg.ChannelID, err)
		c.sendError("server message history failed")
		return
	}

	c.send(protocol.Control{
		Type: "ACK", ChannelID: msg.ChannelID, ServerMessages: out,
		ServerHasMore: hasMore, HasMore: &hasMore, UnreadCount: unread,
	})
}

func (s *Server) handleUpdateServerMessage(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerMessageID <= 0 {
		c.sendError("server message id required")
		return
	}
	if err := encryptedOnly(msg); err != nil {
		c.sendError(err.Error())
		return
	}
	if !serverMessageLimitersFor(c.sess.UserID).edit.Allow() {
		c.sendError("server message rate limit exceeded")
		return
	}

	ctx, cancel := storeCtx()
	defer cancel()
	edited, err := s.messages.EditChannelMessage(ctx, c.sess.UserID, int(msg.ServerMessageID), msg.E2EE)
	if err != nil {
		sendServerMessageError(c, "edit", c.sess.UserID, err)
		return
	}
	summary := messageSummary(edited.Wire())
	s.notifyServer(edited.ServerID, c.sess, protocol.Control{
		Type: "SERVER_MESSAGE_EDITED", ServerID: edited.ServerID,
		ChannelID: int(edited.ChannelID), ServerInfo: &summary,
	})
	c.send(protocol.Control{Type: "ACK", ServerInfo: &summary})
}

func (s *Server) handleDeleteServerMessage(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ServerMessageID <= 0 {
		c.sendError("server message id required")
		return
	}
	ctx, cancel := storeCtx()
	defer cancel()
	deleted, err := s.messages.DeleteChannelMessage(ctx, c.sess.UserID, int(msg.ServerMessageID))
	if err != nil {
		sendServerMessageError(c, "delete", c.sess.UserID, err)
		return
	}
	s.notifyServer(deleted.ServerID, c.sess, protocol.Control{
		Type: "SERVER_MESSAGE_DELETED", ServerID: deleted.ServerID,
		ChannelID: int(deleted.ChannelID), ServerMessageID: protocol.MessageID(deleted.ID),
	})
	c.send(protocol.Control{Type: "ACK", ServerMessageID: protocol.MessageID(deleted.ID)})
}

func (s *Server) handleMarkServerMessageRead(c *client, msg protocol.Control) {
	if !c.requireAuth() {
		return
	}
	if msg.ChannelID <= 0 || msg.ServerMessageID <= 0 {
		c.sendError("invalid server message read")
		return
	}
	channelID := messages.ChannelID(msg.ChannelID)
	ctx, cancel := storeCtx()
	defer cancel()
	watermark, err := s.messages.MarkChannelRead(ctx, c.sess.UserID, channelID, int(msg.ServerMessageID))
	if err != nil {
		sendServerMessageError(c, "mark read", c.sess.UserID, err)
		return
	}
	// No fan-out: read state is recomputed on the server and delivered in
	// LIST_CHANNELS / the next history ACK.
	unread, err := s.messages.UnreadCountForChannel(ctx, c.sess.UserID, channelID)
	if err != nil {
		sendServerMessageError(c, "mark read", c.sess.UserID, err)
		return
	}
	c.send(protocol.Control{Type: "ACK", ServerMessageID: protocol.MessageID(watermark), UnreadCount: unread})
}
