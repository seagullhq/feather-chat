package messages

import (
	"context"
	"database/sql"
	"errors"
	"time"
)

// ChannelID is server_channels.id: the only channel ID that crosses the wire.
// ConversationID is the internal id of the shared conversation tables. Keeping
// them distinct makes it a compile error to put an internal ID on the wire.
type ChannelID int64
type ConversationID int64

var (
	ErrServerChannelNotFound = errors.New("server channel not found")
	ErrServerMessageNotFound = errors.New("server message not found")
)

// MessageSummary is one server text message. ServerID is derived from the
// view so handlers never have to trust the serverId sent by the client.
type MessageSummary struct {
	ID        int
	ChannelID ChannelID
	ServerID  int
	AuthorID  int
	Content   string
	ReplyTo   int
	CreatedAt time.Time
	EditedAt  *time.Time
	DeletedAt *time.Time
}

// DeletedMessage is the identity a soft-delete ACK and push need. Keeping the
// three IDs in one value prevents mixing them up at the call site.
type DeletedMessage struct {
	ID        int
	ChannelID ChannelID
	ServerID  int
}

const (
	defaultChannelPage = 50
	maxChannelPage     = 100
)

// ChannelPageLimit applies the shared page bounds. Handlers and the store call
// it, so a size requested on the wire can never silently become a different
// page size inside the store.
func ChannelPageLimit(requested int) int {
	if requested <= 0 || requested > maxChannelPage {
		return defaultChannelPage
	}
	return requested
}

// unreadCapped is the saturating count shared by the whole-sidebar badge query
// and the single-channel query. $2 is the asking user in both statements: their
// own messages never count as unread.
const unreadCapped = `
		CROSS JOIN LATERAL (
			SELECT count(*) AS unread FROM (
				SELECT 1
				FROM conversation_messages m
				WHERE m.conversation_id = v.conversation_id
				  AND m.id > COALESCE(cm.last_read_message_id, 0)
				  AND m.deleted_at IS NULL
				  AND m.author_id IS DISTINCT FROM $2
				LIMIT 100
			) capped
		) u`

type messageScanner interface {
	Scan(dest ...any) error
}

func scanMessageSummary(row messageScanner) (MessageSummary, error) {
	var m MessageSummary
	var editedAt, deletedAt sql.NullTime
	err := row.Scan(&m.ID, &m.ChannelID, &m.ServerID, &m.AuthorID, &m.Content,
		&m.ReplyTo, &m.CreatedAt, &editedAt, &deletedAt)
	if err != nil {
		return m, err
	}
	if editedAt.Valid {
		m.EditedAt = &editedAt.Time
	}
	if deletedAt.Valid {
		m.DeletedAt = &deletedAt.Time
	}
	return m, nil
}

// assertServerChannelReadable gates a read before it touches the messages of
// a channel. It is a plain table-function query, so an unauthorized caller
// cannot tell an empty channel from a rejected one.
func assertServerChannelReadable(ctx context.Context, q queryer, userID int, ch ChannelID) error {
	var one int
	err := q.QueryRowContext(ctx, `
		SELECT 1 FROM readable_server_channels($2)
		WHERE server_channel_id = $1`, int64(ch), userID).Scan(&one)
	if errors.Is(err, sql.ErrNoRows) {
		return ErrServerChannelNotFound
	}
	return err
}

// resolveServerConversation creates the conversation on first use, with the
// access gate inside the INSERT. A data-modifying CTE cannot be used here:
// WITH clauses share one snapshot, so the view would not see a row inserted
// by the same statement. Once the row exists (also after a lost race) the
// normal SELECT is the only query on subsequent messages.
func resolveServerConversation(ctx context.Context, q queryer, userID, serverID int, ch ChannelID) (ConversationID, error) {
	var convID ConversationID
	err := q.QueryRowContext(ctx, `
		SELECT id FROM conversations WHERE server_channel_id = $1`, int64(ch)).Scan(&convID)
	if err == nil {
		return convID, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return 0, err
	}

	err = q.QueryRowContext(ctx, `
		INSERT INTO conversations (kind, server_channel_id)
		SELECT 'server', r.server_channel_id
		FROM readable_server_channels($2) r
		WHERE r.server_channel_id = $1 AND r.server_id = $3
		ON CONFLICT (server_channel_id) DO NOTHING
		RETURNING id`, int64(ch), userID, serverID).Scan(&convID)
	if err == nil {
		return convID, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return 0, err
	}

	// DO NOTHING returns no row on both a lost race and a denied gate. The
	// SELECT tells them apart: a row means someone else created it.
	err = q.QueryRowContext(ctx, `
		SELECT id FROM conversations WHERE server_channel_id = $1`, int64(ch)).Scan(&convID)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, ErrServerChannelNotFound
	}
	return convID, err
}

// SendChannelMessage stores a message, creating the conversation lazily.
// READ COMMITTED is intentional: after INSERT ... DO NOTHING waits for a
// concurrent creator, the fallback SELECT sees the committed row.
func (s *ServerStore) SendChannelMessage(ctx context.Context, userID, serverID int, ch ChannelID, content string, replyTo int) (*MessageSummary, error) {
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()

	convID, err := resolveServerConversation(ctx, tx, userID, serverID, ch)
	if err != nil {
		return nil, err
	}

	// Check access on every send, not just on creation: the sender may have
	// been kicked since, and serverId/channelId both come from the client.
	var wireChannel ChannelID
	var wireServer int
	err = tx.QueryRowContext(ctx, `
		SELECT v.server_channel_id, v.server_id
		FROM server_chat_channels v
		JOIN readable_server_channels($3) r ON r.server_channel_id = v.server_channel_id
		WHERE v.conversation_id = $1 AND r.server_id = $2`,
		int64(convID), serverID, userID).Scan(&wireChannel, &wireServer)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrServerChannelNotFound
	}
	if err != nil {
		return nil, err
	}

	var m MessageSummary
	var reply int
	err = tx.QueryRowContext(ctx, `
		INSERT INTO conversation_messages (conversation_id, author_id, content, reply_to)
		SELECT $1, $2, $3, NULLIF($4, 0)
		WHERE $4 = 0 OR EXISTS (
			SELECT 1 FROM conversation_messages p
			WHERE p.id = $4 AND p.conversation_id = $1 AND p.deleted_at IS NULL)
		RETURNING id, COALESCE(reply_to, 0), created_at`,
		int64(convID), userID, content, replyTo).
		Scan(&m.ID, &reply, &m.CreatedAt)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrServerMessageNotFound // reply outside the channel or deleted
	}
	if err != nil {
		return nil, err
	}
	m.ChannelID = wireChannel
	m.ServerID = wireServer
	m.AuthorID = userID
	m.Content = content
	m.ReplyTo = reply

	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return &m, nil
}

// ListChannelMessages returns one newest-first page of a channel's history.
// beforeID is only a keyset cursor: the access gate is the table function.
//
// The conversation is resolved first, then messages are read without joining
// the view. A join would make the planner add a Sort: it cannot prove that
// the outer side has one row, so it cannot guarantee global id order across
// its rows. The direct query uses the keyset index backwards instead.
func (s *ServerStore) ListChannelMessages(ctx context.Context, userID int, ch ChannelID, before int64, limit int) ([]MessageSummary, error) {
	limit = ChannelPageLimit(limit)
	if err := assertServerChannelReadable(ctx, s.db, userID, ch); err != nil {
		return nil, err
	}

	var convID ConversationID
	var serverID int
	err := s.db.QueryRowContext(ctx, `
		SELECT c.id, sc.server_id
		FROM conversations c
		JOIN server_channels sc ON sc.id = c.server_channel_id
		JOIN channel_types t ON t.id = sc.channel_type AND t.type_name = 'thread'
		WHERE c.kind = 'server' AND c.server_channel_id = $1`, int64(ch)).Scan(&convID, &serverID)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil // readable channel with no messages yet
	}
	if err != nil {
		return nil, err
	}

	// One extra row is the has-more sentinel; the handler slices to limit.
	rows, err := s.db.QueryContext(ctx, `
		SELECT m.id, $2::bigint, $3::bigint, COALESCE(m.author_id, 0),
		       m.content, COALESCE(m.reply_to, 0), m.created_at, m.edited_at, m.deleted_at
		FROM conversation_messages m
		WHERE m.conversation_id = $1
		  AND m.deleted_at IS NULL
		  AND ($4 = 0 OR m.id < $4)
		ORDER BY m.id DESC
		LIMIT $5`, int64(convID), int64(ch), serverID, before, limit+1)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []MessageSummary
	for rows.Next() {
		m, err := scanMessageSummary(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, m)
	}
	return out, rows.Err()
}

// EditChannelMessage changes an authored, non-deleted message. The author
// predicate is in the UPDATE itself, so authorization survives refactors.
func (s *ServerStore) EditChannelMessage(ctx context.Context, userID int, messageID int, content string) (*MessageSummary, error) {
	m, err := scanMessageSummary(s.db.QueryRowContext(ctx, `
		UPDATE conversation_messages m
		SET content = $3, edited_at = now()
		FROM server_chat_channels v
		WHERE m.id = $1
		  AND m.conversation_id = v.conversation_id
		  AND m.author_id = $2
		  AND m.deleted_at IS NULL
		  AND can_read_server_channel($2, v.server_channel_id)
		RETURNING m.id, v.server_channel_id, v.server_id, COALESCE(m.author_id, 0),
		          m.content, COALESCE(m.reply_to, 0), m.created_at, m.edited_at, m.deleted_at`,
		messageID, userID, content))
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrServerMessageNotFound
	}
	if err != nil {
		return nil, err
	}
	return &m, nil
}

// DeleteChannelMessage soft-deletes an authored message and blanks its
// content, freeing TOAST chunks. It returns the identity needed for the ACK
// and the push.
func (s *ServerStore) DeleteChannelMessage(ctx context.Context, userID int, messageID int) (*DeletedMessage, error) {
	var m DeletedMessage
	err := s.db.QueryRowContext(ctx, `
		UPDATE conversation_messages m
		SET content = '', deleted_at = now()
		FROM server_chat_channels v
		WHERE m.id = $1
		  AND m.conversation_id = v.conversation_id
		  AND m.author_id = $2
		  AND m.deleted_at IS NULL
		  AND can_read_server_channel($2, v.server_channel_id)
		RETURNING m.id, v.server_channel_id, v.server_id`,
		messageID, userID).Scan(&m.ID, &m.ChannelID, &m.ServerID)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrServerMessageNotFound
	}
	if err != nil {
		return nil, err
	}
	return &m, nil
}

// MarkChannelRead advances the read pointer monotonically. The message must
// belong to the requested channel and the caller must still be able to read
// it; 0 rows means either check failed.
func (s *ServerStore) MarkChannelRead(ctx context.Context, userID int, ch ChannelID, upToID int) (int, error) {
	var lastRead int
	err := s.db.QueryRowContext(ctx, `
		INSERT INTO conversation_members (conversation_id, user_id, last_read_message_id)
		SELECT m.conversation_id, $2, m.id
		FROM conversation_messages m
		JOIN server_chat_channels v ON v.conversation_id = m.conversation_id
		JOIN readable_server_channels($2) r ON r.server_channel_id = v.server_channel_id
		WHERE m.id = $3 AND v.server_channel_id = $1
		ON CONFLICT (conversation_id, user_id) DO UPDATE
		  SET last_read_message_id = GREATEST(conversation_members.last_read_message_id,
		                                      EXCLUDED.last_read_message_id)
		RETURNING last_read_message_id`, int64(ch), userID, upToID).Scan(&lastRead)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, ErrServerMessageNotFound
	}
	return lastRead, err
}

// UnreadCountsForUser returns one saturated (0..100) count per readable
// channel of a server in a single query. 100 means "100 or more".
func (s *ServerStore) UnreadCountsForUser(ctx context.Context, userID, serverID int) (map[ChannelID]int, error) {
	rows, err := s.db.QueryContext(ctx, `
		SELECT v.server_channel_id, u.unread
		FROM server_chat_channels v
		JOIN readable_server_channels($2) r ON r.server_channel_id = v.server_channel_id
		LEFT JOIN conversation_members cm
		  ON cm.conversation_id = v.conversation_id AND cm.user_id = $2
		`+unreadCapped+`
		WHERE v.server_id = $1`, serverID, userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	out := make(map[ChannelID]int)
	for rows.Next() {
		var ch ChannelID
		var unread int64
		if err := rows.Scan(&ch, &unread); err != nil {
			return nil, err
		}
		out[ch] = int(unread)
	}
	return out, rows.Err()
}

// UnreadCountForChannel is the same saturated count for one channel, used to
// report the count after MarkChannelRead advanced the pointer.
func (s *ServerStore) UnreadCountForChannel(ctx context.Context, userID int, ch ChannelID) (int, error) {
	if err := assertServerChannelReadable(ctx, s.db, userID, ch); err != nil {
		return 0, err
	}
	var unread int64
	err := s.db.QueryRowContext(ctx, `
		SELECT u.unread
		FROM server_chat_channels v
		JOIN readable_server_channels($2) r ON r.server_channel_id = v.server_channel_id
		LEFT JOIN conversation_members cm
		  ON cm.conversation_id = v.conversation_id AND cm.user_id = $2
		`+unreadCapped+`
		WHERE v.server_channel_id = $1`, int64(ch), userID).Scan(&unread)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, nil // readable but no conversation yet
	}
	return int(unread), err
}
