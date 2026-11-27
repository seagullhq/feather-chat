package messages

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"github.com/jackc/pgx/v5/pgconn"

	"github.com/seagullhq/feather-chat/server/internal/e2ee"
)

// ServerStore persists servers and their content.
type ServerStore struct {
	db *sql.DB
}

// NewStore wires a ServerStore onto an open database.
func NewStore(db *sql.DB) *ServerStore {
	return &ServerStore{db: db}
}

// execQueryer is what *sql.DB and *sql.Tx share to open a conversation.
type execQueryer interface {
	queryer
	ExecContext(context.Context, string, ...any) (sql.Result, error)
}

// queryer is the shared slice of *sql.DB / *sql.Tx used by the DM helpers.
type queryer interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
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
	// E2EE is set on encrypted messages, whose Content is empty.
	E2EE *e2ee.Message
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
	E2EE          *e2ee.Message
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

// SendDM stores a plaintext message. Clients cannot: only call events write
// plaintext, and tests seeding rows older than encryption.
func (s *ServerStore) SendDM(ctx context.Context, senderID, recipientID, replyToID int, content string) (*DirectMessage, error) {
	return sendDM(ctx, s.db, senderID, recipientID, replyToID, content, "", false, nil)
}

// SendEncryptedDM checks and stores an encrypted message.
func (s *ServerStore) SendEncryptedDM(ctx context.Context, senderID, recipientID, replyToID int, m *e2ee.Message) (dm *DirectMessage, err error) {
	err = s.withConversation(ctx, senderID, Address{PeerID: recipientID}, func(tx *sql.Tx, conv int64) error {
		if err := checkNew(ctx, tx, conv, senderID, m); err != nil {
			return err
		}
		dm, err = sendDM(ctx, tx, senderID, recipientID, replyToID, "", "", false, m)
		return err
	})
	if err != nil {
		return nil, err
	}
	return dm, nil
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
	return sendDM(ctx, s.db, callerID, calleeID, 0, "Call "+outcome, string(call), readByCallee, nil)
}

// sendDM is the one insert behind SendDM and RecordCall. callJSON "" means a
// text message; readByRecipient also moves the recipient's read pointer.
func sendDM(ctx context.Context, q queryer, senderID, recipientID, replyToID int, content, callJSON string, readByRecipient bool, enc *e2ee.Message) (*DirectMessage, error) {
	if senderID == recipientID {
		return nil, ErrSelfRequest
	}

	low, high := pair(senderID, recipientID)
	if replyToID != 0 {
		var exists int
		err := q.QueryRowContext(ctx, `
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
	args := []any{low, senderID, content, recipientID, replyToID, high, callJSON, readByRecipient,
		nil, nil, nil, nil, nil, nil}
	if enc != nil {
		copy(args[8:], []any{int64(enc.Epoch), enc.MsgID, int64(enc.Revision), enc.SenderKeyID, enc.Box, enc.MsgSig})
	}
	err := q.QueryRowContext(ctx, `
		WITH ch AS (
			INSERT INTO conversations (kind, user_low, user_high) VALUES ('direct', $1, $6)
			ON CONFLICT (user_low, user_high) DO UPDATE SET updated_at = now()
			RETURNING id
		), msg AS (
			INSERT INTO conversation_messages (conversation_id, author_id, content, reply_to, call,
				e2ee_epoch, e2ee_msg_id, e2ee_revision, e2ee_sender_key, e2ee_box, e2ee_sig, e2ee_sender)
			SELECT id, $2, $3, NULLIF($5, 0), NULLIF($7::text, '')::jsonb,
				$9::int, $10::bytea, $11::bigint, $12::bigint, $13::bytea, $14::bytea,
				CASE WHEN $13::bytea IS NOT NULL THEN $2::bigint END FROM ch
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
		SELECT * FROM msg`, args...).
		Scan(&m.ID, &m.ChannelID, &m.Author, &m.Content, &m.ReplyTo, &m.CreatedAt,
			&m.CallOutcome, &m.CallSeconds)
	if err != nil {
		return nil, mapPgError(err)
	}
	m.E2EE = enc
	return &m, nil
}

const (
	defaultDMPage = 50
	// ConversationPage bounds one page of the DM sidebar.
	ConversationPage = 200
	maxDMPage        = 100
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
		SELECT m.id, m.conversation_id, COALESCE(m.author_id, m.e2ee_sender, 0), m.content, COALESCE(m.reply_to, 0),
		       m.created_at, m.edited_at, m.deleted_at,
		       COALESCE(m.call->>'outcome', ''), COALESCE((m.call->>'seconds')::int, 0),
		       `+e2eeColumns+`
		FROM conversations c
		JOIN conversation_members member ON member.conversation_id = c.id AND member.user_id = $1
		JOIN conversation_messages m ON m.conversation_id = c.id
		WHERE c.user_low = $2 AND c.user_high = $3 AND m.deleted_at IS NULL AND ($4 = 0 OR m.id < $4)
		ORDER BY m.id DESC
		LIMIT $5`, viewerID, low, high, beforeID, limit+1) // one extra: has more
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []DirectMessage
	for rows.Next() {
		var m DirectMessage
		var editedAt, deletedAt sql.NullTime
		var enc e2eeRow
		if err := rows.Scan(append([]any{&m.ID, &m.ChannelID, &m.Author, &m.Content, &m.ReplyTo,
			&m.CreatedAt, &editedAt, &deletedAt, &m.CallOutcome, &m.CallSeconds}, enc.dest()...)...); err != nil {
			return nil, err
		}
		m.E2EE = enc.message()
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

// EditDM replaces the ciphertext of an authored, non-deleted message. The
// second return is the other member of the conversation, for push notification.
func (s *ServerStore) EditDM(ctx context.Context, userID, messageID int, enc *e2ee.Message) (*DirectMessage, int, error) {
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return nil, 0, err
	}
	defer tx.Rollback()

	// Lock the conversation, then read the message under that lock.
	var conv int64
	var peerID int
	err = tx.QueryRowContext(ctx, `
		SELECT c.id, CASE WHEN c.user_low = $2 THEN c.user_high ELSE c.user_low END
		FROM conversations c
		JOIN conversation_messages m ON m.conversation_id = c.id
		WHERE m.id = $1 AND m.author_id = $2 AND c.kind = 'direct'
		FOR UPDATE OF c`, messageID, userID).Scan(&conv, &peerID)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, 0, ErrMessageNotFound
	}
	if err != nil {
		return nil, 0, err
	}
	var old e2eeRow
	err = tx.QueryRowContext(ctx, `
		SELECT `+e2eeColumns+` FROM conversation_messages m
		WHERE m.id = $1 AND m.deleted_at IS NULL AND m.call IS NULL`, messageID).Scan(old.dest()...)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, 0, ErrMessageNotFound
	}
	if err != nil {
		return nil, 0, err
	}
	if err := checkMessage(ctx, tx, conv, userID, enc); err != nil {
		return nil, 0, err
	}
	if err := checkEdit(old, enc); err != nil {
		return nil, 0, err
	}

	var m DirectMessage
	var editedAt sql.NullTime
	err = tx.QueryRowContext(ctx, `
		UPDATE conversation_messages SET edited_at = now(), e2ee_epoch = $2, e2ee_revision = $3,
			e2ee_sender_key = $4, e2ee_box = $5, e2ee_sig = $6
		WHERE id = $1
		RETURNING id, conversation_id, author_id, content, COALESCE(reply_to, 0), created_at, edited_at`,
		messageID, int64(enc.Epoch), int64(enc.Revision), enc.SenderKeyID, enc.Box, enc.MsgSig).
		Scan(&m.ID, &m.ChannelID, &m.Author, &m.Content, &m.ReplyTo, &m.CreatedAt, &editedAt)
	if err != nil {
		return nil, 0, err
	}
	m.EditedAt, m.E2EE = &editedAt.Time, enc

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
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return 0, 0, err
	}
	defer tx.Rollback()

	// Lock the conversation before the message, as EditDM does: the other order
	// deadlocks.
	if _, err := tx.ExecContext(ctx, `
		SELECT c.id FROM conversations c
		JOIN conversation_messages m ON m.conversation_id = c.id
		WHERE m.id = $1 AND m.author_id = $2 AND c.kind = 'direct'
		FOR UPDATE OF c`, messageID, userID); err != nil {
		return 0, 0, err
	}
	var id, channelID, peerID int
	err = tx.QueryRowContext(ctx, `
		UPDATE conversation_messages m SET deleted_at = now(), content = '', e2ee_box = NULL, e2ee_sig = NULL
		FROM conversations c
		WHERE m.id = $1 AND m.author_id = $2 AND m.deleted_at IS NULL AND m.call IS NULL
		  AND c.id = m.conversation_id AND c.kind = 'direct'
		RETURNING m.id, c.id, CASE WHEN c.user_low = $2 THEN c.user_high ELSE c.user_low END`,
		messageID, userID).Scan(&id, &channelID, &peerID)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, 0, ErrMessageNotFound
	}
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
		WHERE conversation_id = $1 AND author_id IS DISTINCT FROM $2 AND deleted_at IS NULL
		  AND id > $3`, channelID.Int64, viewerID, state.LastReadMessageID).
		Scan(&state.UnreadCount)
	return state, err
}

// ListDMConversations returns the viewer's DM channels, most recently active
// first, with the last message and unread count of each.
// before (0 = newest) is the channelId of the last entry of the previous page.
// At most ConversationPage+1 entries are returned.
func (s *ServerStore) ListDMConversations(ctx context.Context, viewerID, before int) ([]DMConversation, error) {
	rows, err := s.db.QueryContext(ctx, `
		SELECT c.id, peer.user_id,
		       COALESCE(m.id, 0), COALESCE(m.content, ''),
		       COALESCE(m.author_id, m.e2ee_sender, 0), COALESCE(m.created_at, c.created_at),
		       (SELECT COUNT(*) FROM conversation_messages unread
		        WHERE unread.conversation_id = c.id AND unread.author_id IS DISTINCT FROM $1
		          AND unread.deleted_at IS NULL
		          AND unread.id > COALESCE(member.last_read_message_id, 0)),
		       `+e2eeColumns+`
		FROM conversations c
		JOIN conversation_members member ON member.conversation_id = c.id AND member.user_id = $1
		JOIN conversation_members peer ON peer.conversation_id = c.id AND peer.user_id <> $1
		LEFT JOIN conversation_messages m ON m.id = (
			SELECT id FROM conversation_messages
			WHERE conversation_id = c.id AND deleted_at IS NULL
			ORDER BY id DESC LIMIT 1
		)
		WHERE c.kind = 'direct' AND NOT EXISTS (
			SELECT 1 FROM friendships f
			WHERE f.status = 'BLOCKED'
			  AND ((f.requester_id = $1 AND f.addressee_id = peer.user_id)
			    OR (f.requester_id = peer.user_id AND f.addressee_id = $1))
		) AND ($2::bigint = 0 OR (c.updated_at, c.id) < (
			SELECT b.updated_at, b.id FROM conversations b
			WHERE b.id = $2::bigint AND (b.user_low = $1 OR b.user_high = $1)))
		ORDER BY c.updated_at DESC, c.id DESC
		LIMIT $3`, viewerID, before, ConversationPage+1)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []DMConversation
	for rows.Next() {
		var conv DMConversation
		var enc e2eeRow
		if err := rows.Scan(append([]any{&conv.ChannelID, &conv.PeerID, &conv.LastMessageID, &conv.Content,
			&conv.AuthorID, &conv.CreatedAt, &conv.UnreadCount}, enc.dest()...)...); err != nil {
			return nil, err
		}
		conv.E2EE = enc.message()
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
			if pgErr.ConstraintName == "conversation_messages_msg_id_idx" {
				return ErrDuplicateID
			}
			return ErrAlreadyExists
		case "23514":
			return ErrSelfRequest
		case "23503":
			return ErrNotFound
		}
	}
	return err
}
