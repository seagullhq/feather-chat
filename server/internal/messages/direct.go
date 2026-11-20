package messages

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
)

// ServerStore persists servers and their content.
type ServerStore struct {
	db *sql.DB
}

// NewStore wires a ServerStore onto an open database.
func NewStore(db *sql.DB) *ServerStore {
	return &ServerStore{db: db}
}

// queryer is the shared slice of *sql.DB / *sql.Tx used by the DM helpers.
type queryer interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

// otherMember returns the peer of a two-person DM channel.
func otherMember(ctx context.Context, q queryer, channelID, userID int) (int, error) {
	var peerID int
	err := q.QueryRowContext(ctx, `
		SELECT user_id FROM conversation_members
		WHERE conversation_id = $1 AND user_id <> $2
		ORDER BY user_id LIMIT 1`, channelID, userID).Scan(&peerID)
	return peerID, err
}

var (
	ErrNotFound        = errors.New("DM channel not found")
	ErrSelfRequest     = errors.New("cannot send DM to yourself")
	ErrAlreadyExists   = errors.New("DM already exists")
	ErrMessageNotFound = errors.New("direct message not found")
)

type DirectMessage struct {
	ID        int
	ChannelID int
	Author    int
	Content   string
	ReplyTo   int
	CreatedAt time.Time
	EditedAt  *time.Time
	DeletedAt *time.Time
	// CallOutcome and CallSeconds come from the `call` JSON column: set on a
	// call event (ended, rejected, cancelled, missed, offline), empty on text.
	CallOutcome string
	CallSeconds int
}

// DMConversation is one entry of a user's DM sidebar.
type DMConversation struct {
	ChannelID     int
	PeerID        int
	LastMessageID int
	Content       string
	AuthorID      int
	CreatedAt     time.Time
	UnreadCount   int
}

// ReadState is the viewer's read pointer for a conversation after marking it.
type ReadState struct {
	LastReadMessageID int
	UnreadCount       int
	Changed           bool
}

// pair orders two users the way conversations store them (user_low < user_high),
// so (a, b) and (b, a) are the same conversation.
func pair(a, b int) (low, high int) {
	return min(a, b), max(a, b)
}

// SendDM stores a message from senderID to recipientID, creating the
// conversation on first use. The sender has read their own message; the
// recipient's read pointer is untouched.
func (s *ServerStore) SendDM(ctx context.Context, senderID, recipientID, replyToID int, content string) (*DirectMessage, error) {
	return s.sendDM(ctx, senderID, recipientID, replyToID, content, "", false)
}

// RecordCall logs a finished 1:1 call in the pair's conversation, authored by
// the caller. The caller has read it; the callee too when they took part
// (ended, rejected), else it stays unread (missed, cancelled, offline).
func (s *ServerStore) RecordCall(ctx context.Context, callerID, calleeID int, outcome string, seconds int) (*DirectMessage, error) {
	readByCallee := outcome == "ended" || outcome == "rejected"
	call, err := json.Marshal(struct {
		Outcome string `json:"outcome"`
		Seconds int    `json:"seconds,omitempty"`
	}{outcome, seconds})
	if err != nil {
		return nil, err
	}
	return s.sendDM(ctx, callerID, calleeID, 0, "Call "+outcome, string(call), readByCallee)
}

// sendDM is the one insert behind SendDM and RecordCall. callJSON "" means a
// text message; readByRecipient also moves the recipient's read pointer.
func (s *ServerStore) sendDM(ctx context.Context, senderID, recipientID, replyToID int, content, callJSON string, readByRecipient bool) (*DirectMessage, error) {
	if senderID == recipientID {
		return nil, ErrSelfRequest
	}

	low, high := pair(senderID, recipientID)
	if replyToID != 0 {
		var exists int
		err := s.db.QueryRowContext(ctx, `
			SELECT 1 FROM conversation_messages m
			JOIN conversations c ON c.id = m.conversation_id
			WHERE m.id = $1 AND c.user_low = $2 AND c.user_high = $3 AND m.deleted_at IS NULL`,
			replyToID, low, high).
			Scan(&exists)
		if errors.Is(err, sql.ErrNoRows) {
			return nil, ErrMessageNotFound
		}
		if err != nil {
			return nil, err
		}
	}

	// One statement, so it is atomic. The channel upsert uses DO UPDATE (not
	// DO NOTHING) so it always returns the row, even if a concurrent first
	// message created it, and it bumps the sidebar order. GREATEST ignores NULL:
	// the recipient's read pointer stays as it was.
	var m DirectMessage
	err := s.db.QueryRowContext(ctx, `
		WITH ch AS (
			INSERT INTO conversations (kind, user_low, user_high) VALUES ('direct', $1, $6)
			ON CONFLICT (user_low, user_high) DO UPDATE SET updated_at = now()
			RETURNING id
		), msg AS (
			INSERT INTO conversation_messages (conversation_id, author_id, content, reply_to, call)
			SELECT id, $2, $3, NULLIF($5, 0), NULLIF($7::text, '')::jsonb FROM ch
			RETURNING id, conversation_id, author_id, content, COALESCE(reply_to, 0), created_at,
			          COALESCE(call->>'outcome', ''), COALESCE((call->>'seconds')::int, 0)
		), sender AS (
			-- Sender: member of the conversation, and has read their own message.
			INSERT INTO conversation_members (conversation_id, user_id, last_read_message_id)
			SELECT conversation_id, $2, id FROM msg
			ON CONFLICT (conversation_id, user_id) DO UPDATE SET last_read_message_id =
				GREATEST(conversation_members.last_read_message_id, EXCLUDED.last_read_message_id)
		), recipient AS (
			-- Recipient: becomes a member so the chat shows in their sidebar.
			-- Nothing read yet (unless $9); GREATEST ignores NULL, so an
			-- existing read pointer is left alone.
			INSERT INTO conversation_members (conversation_id, user_id, last_read_message_id)
			SELECT conversation_id, $4, CASE WHEN $8::bool THEN id END FROM msg
			ON CONFLICT (conversation_id, user_id) DO UPDATE SET last_read_message_id =
				GREATEST(conversation_members.last_read_message_id, EXCLUDED.last_read_message_id)
		)
		SELECT * FROM msg`, low, senderID, content, recipientID, replyToID, high,
		callJSON, readByRecipient).
		Scan(&m.ID, &m.ChannelID, &m.Author, &m.Content, &m.ReplyTo, &m.CreatedAt,
			&m.CallOutcome, &m.CallSeconds)
	if err != nil {
		return nil, mapPgError(err)
	}
	return &m, nil
}

const (
	defaultDMPage = 50
	maxDMPage     = 100
)

// ListDM returns the conversation between viewerID and peerID, newest first.
// The membership join keeps a caller from reading a channel it is not part of,
// and beforeID enables keyset pagination (0 asks for the newest page).
func (s *ServerStore) ListDM(ctx context.Context, viewerID, peerID, limit, beforeID int) ([]DirectMessage, error) {
	if viewerID == peerID {
		return nil, ErrSelfRequest
	}
	if limit <= 0 || limit > maxDMPage {
		limit = defaultDMPage
	}
	low, high := pair(viewerID, peerID)
	rows, err := s.db.QueryContext(ctx, `
		SELECT m.id, m.conversation_id, m.author_id, m.content, COALESCE(m.reply_to, 0),
		       m.created_at, m.edited_at, m.deleted_at,
		       COALESCE(m.call->>'outcome', ''), COALESCE((m.call->>'seconds')::int, 0)
		FROM conversations c
		JOIN conversation_members member ON member.conversation_id = c.id AND member.user_id = $1
		JOIN conversation_messages m ON m.conversation_id = c.id
		WHERE c.user_low = $2 AND c.user_high = $3 AND m.deleted_at IS NULL AND ($4 = 0 OR m.id < $4)
		ORDER BY m.id DESC
		LIMIT $5`, viewerID, low, high, beforeID, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []DirectMessage
	for rows.Next() {
		var m DirectMessage
		var editedAt, deletedAt sql.NullTime
		if err := rows.Scan(&m.ID, &m.ChannelID, &m.Author, &m.Content, &m.ReplyTo,
			&m.CreatedAt, &editedAt, &deletedAt, &m.CallOutcome, &m.CallSeconds); err != nil {
			return nil, err
		}
		if editedAt.Valid {
			m.EditedAt = &editedAt.Time
		}
		if deletedAt.Valid {
			m.DeletedAt = &deletedAt.Time
		}
		out = append(out, m)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return out, nil
}

// EditDM changes the content of an authored, non-deleted message. The second
// return is the other member of the conversation, for push notification.
func (s *ServerStore) EditDM(ctx context.Context, userID, messageID int, content string) (*DirectMessage, int, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, 0, err
	}
	defer tx.Rollback()

	var m DirectMessage
	var editedAt sql.NullTime
	err = tx.QueryRowContext(ctx, `
		UPDATE conversation_messages SET content = $3, edited_at = now()
		WHERE id = $1 AND author_id = $2 AND deleted_at IS NULL AND call IS NULL
		RETURNING id, conversation_id, author_id, content, COALESCE(reply_to, 0),
		          created_at, edited_at, deleted_at`, messageID, userID, content).
		Scan(&m.ID, &m.ChannelID, &m.Author, &m.Content, &m.ReplyTo,
			&m.CreatedAt, &editedAt, &m.DeletedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, 0, ErrMessageNotFound
	}
	if err != nil {
		return nil, 0, err
	}
	m.EditedAt = &editedAt.Time

	peerID, err := otherMember(ctx, tx, m.ChannelID, userID)
	if err != nil {
		return nil, 0, err
	}
	if _, err := tx.ExecContext(ctx, `UPDATE conversations SET updated_at = now() WHERE id = $1`, m.ChannelID); err != nil {
		return nil, 0, err
	}
	if err := tx.Commit(); err != nil {
		return nil, 0, err
	}
	return &m, peerID, nil
}

// DeleteDM soft-deletes an authored message and returns its ID and the other
// member of the conversation for push notification.
func (s *ServerStore) DeleteDM(ctx context.Context, userID, messageID int) (int, int, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return 0, 0, err
	}
	defer tx.Rollback()

	var id, channelID int
	err = tx.QueryRowContext(ctx, `
		UPDATE conversation_messages SET deleted_at = now()
		WHERE id = $1 AND author_id = $2 AND deleted_at IS NULL AND call IS NULL
		RETURNING id, conversation_id`, messageID, userID).Scan(&id, &channelID)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, 0, ErrMessageNotFound
	}
	if err != nil {
		return 0, 0, err
	}

	peerID, err := otherMember(ctx, tx, channelID, userID)
	if err != nil {
		return 0, 0, err
	}
	if _, err := tx.ExecContext(ctx, `UPDATE conversations SET updated_at = now() WHERE id = $1`, channelID); err != nil {
		return 0, 0, err
	}
	if err := tx.Commit(); err != nil {
		return 0, 0, err
	}
	return id, peerID, nil
}

// MarkDMRead advances the viewer's read pointer up to upToID (0 only queries)
// and reports how many messages from the peer are still unread.
func (s *ServerStore) MarkDMRead(ctx context.Context, viewerID, peerID, upToID int) (ReadState, error) {
	var state ReadState
	low, high := pair(viewerID, peerID)
	var channelID, lastRead sql.NullInt64
	err := s.db.QueryRowContext(ctx, `
		SELECT c.id, m.last_read_message_id
		FROM conversations c
		JOIN conversation_members m ON m.conversation_id = c.id AND m.user_id = $1
		WHERE c.user_low = $2 AND c.user_high = $3`, viewerID, low, high).
		Scan(&channelID, &lastRead)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return state, nil
		}
		return state, err
	}
	state.LastReadMessageID = int(lastRead.Int64)
	if upToID > state.LastReadMessageID {
		if _, err := s.db.ExecContext(ctx, `
			UPDATE conversation_members
			SET last_read_message_id = GREATEST(COALESCE(last_read_message_id, 0), $3)
			WHERE conversation_id = $1 AND user_id = $2`, channelID.Int64, viewerID, upToID); err != nil {
			return state, err
		}
		state.LastReadMessageID = upToID
		state.Changed = true
	}
	err = s.db.QueryRowContext(ctx, `
		SELECT COUNT(*) FROM conversation_messages
		WHERE conversation_id = $1 AND author_id <> $2 AND deleted_at IS NULL
		  AND id > $3`, channelID.Int64, viewerID, state.LastReadMessageID).
		Scan(&state.UnreadCount)
	return state, err
}

// ListDMConversations returns the viewer's DM channels, most recently active
// first, with the last message and unread count of each.
func (s *ServerStore) ListDMConversations(ctx context.Context, viewerID int) ([]DMConversation, error) {
	rows, err := s.db.QueryContext(ctx, `
		SELECT c.id, peer.user_id,
		       COALESCE(last.id, 0), COALESCE(last.content, ''),
		       COALESCE(last.author_id, 0), COALESCE(last.created_at, c.created_at),
		       (SELECT COUNT(*) FROM conversation_messages unread
		        WHERE unread.conversation_id = c.id AND unread.author_id <> $1
		          AND unread.deleted_at IS NULL
		          AND unread.id > COALESCE(member.last_read_message_id, 0))
		FROM conversations c
		JOIN conversation_members member ON member.conversation_id = c.id AND member.user_id = $1
		JOIN conversation_members peer ON peer.conversation_id = c.id AND peer.user_id <> $1
		LEFT JOIN conversation_messages last ON last.id = (
			SELECT id FROM conversation_messages
			WHERE conversation_id = c.id AND deleted_at IS NULL
			ORDER BY id DESC LIMIT 1
		)
		WHERE c.kind = 'direct' AND NOT EXISTS (
			SELECT 1 FROM friendships f
			WHERE f.status = 'BLOCKED'
			  AND ((f.requester_id = $1 AND f.addressee_id = peer.user_id)
			    OR (f.requester_id = peer.user_id AND f.addressee_id = $1))
		)
		ORDER BY c.updated_at DESC, c.id DESC`, viewerID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []DMConversation
	for rows.Next() {
		var conv DMConversation
		if err := rows.Scan(&conv.ChannelID, &conv.PeerID, &conv.LastMessageID, &conv.Content,
			&conv.AuthorID, &conv.CreatedAt, &conv.UnreadCount); err != nil {
			return nil, err
		}
		out = append(out, conv)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return out, nil
}

func mapPgError(err error) error {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		switch pgErr.Code {
		case "23505":
			return ErrAlreadyExists
		case "23514":
			return ErrSelfRequest
		case "23503":
			return ErrNotFound
		}
	}
	return err
}
