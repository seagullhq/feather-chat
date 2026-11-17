package messages

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
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
		SELECT user_id FROM direct_channel_members
		WHERE channel_id = $1 AND user_id <> $2
		ORDER BY user_id LIMIT 1`, channelID, userID).Scan(&peerID)
	return peerID, err
}

var (
	ErrNotFound        = errors.New("DM channel not found")
	ErrSelfRequest     = errors.New("cannot send DM to yourself")
	ErrAlreadyExists   = errors.New("DM already exists")
	ErrMessageNotFound = errors.New("direct message not found")
)

type DirectChannel struct {
	ID        int
	DMKey     string
	CreatedAt time.Time
	UpdatedAt time.Time
}

type DirectMessage struct {
	ID        int
	ChannelID int
	Author    int
	Content   string
	ReplyTo   int
	CreatedAt time.Time
	EditedAt  *time.Time
	DeletedAt *time.Time
}

type DirectChannelMembers struct {
	ChannelID         int
	UserID            int
	LastReadMessageID int
	JoinedAt          time.Time
}

type Direct struct {
	DirectCh             DirectChannel
	DirectMsg            DirectMessage
	DirectChannelMembers DirectChannelMembers
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

func makeDMKey(requesterID, addresseeID int64) string {
	a, b := min(requesterID, addresseeID), max(requesterID, addresseeID)
	return fmt.Sprintf("%d:%d", a, b)
}

func (s *ServerStore) SendDM(ctx context.Context, requesterID, addresseeID, replyToID int, msg string) (*Direct, error) {
	if requesterID == addresseeID {
		return nil, ErrSelfRequest
	}

	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()

	now := time.Now().UTC()
	dmKey := makeDMKey(int64(requesterID), int64(addresseeID))

	const channelQuery = `
		WITH ins AS (
			INSERT INTO direct_channels (dm_key, created_at, updated_at)
			VALUES ($1, $2, $3)
			ON CONFLICT (dm_key) DO NOTHING
			RETURNING id, dm_key, created_at, updated_at
		)
		SELECT id, dm_key, created_at, updated_at FROM ins
		UNION ALL
		SELECT id, dm_key, created_at, updated_at FROM direct_channels WHERE dm_key = $1
		LIMIT 1
	`
	var directCh DirectChannel
	err = tx.QueryRowContext(ctx, channelQuery, dmKey, now, now).
		Scan(&directCh.ID, &directCh.DMKey, &directCh.CreatedAt, &directCh.UpdatedAt)
	if err != nil {
		return nil, mapPgError(err)
	}

	if replyToID != 0 {
		var exists int
		err = tx.QueryRowContext(ctx, `
			SELECT 1 FROM direct_messages
			WHERE id = $1 AND channel_id = $2 AND deleted_at IS NULL`, replyToID, directCh.ID).
			Scan(&exists)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				return nil, ErrMessageNotFound
			}
			return nil, err
		}
	}

	const msgQuery = `
		INSERT INTO direct_messages (channel_id, author_id, content, reply_to, created_at)
		VALUES ($1, $2, $3, NULLIF($4, 0), $5)
		RETURNING id, channel_id, author_id, content, COALESCE(reply_to, 0), created_at
	`
	var directMsg DirectMessage
	err = tx.QueryRowContext(ctx, msgQuery, directCh.ID, requesterID, msg, replyToID, now).
		Scan(
			&directMsg.ID,
			&directMsg.ChannelID,
			&directMsg.Author,
			&directMsg.Content,
			&directMsg.ReplyTo,
			&directMsg.CreatedAt,
		)
	if err != nil {
		return nil, mapPgError(err)
	}

	const senderMemberQuery = `
		INSERT INTO direct_channel_members (channel_id, user_id, last_read_message_id, joined_at)
		VALUES ($1, $2, $3, $4)
		ON CONFLICT (channel_id, user_id) DO UPDATE
			SET last_read_message_id = GREATEST(
				direct_channel_members.last_read_message_id,
				EXCLUDED.last_read_message_id
			)
		RETURNING channel_id, user_id, last_read_message_id, joined_at
	`
	var senderMember DirectChannelMembers
	err = tx.QueryRowContext(ctx, senderMemberQuery, directCh.ID, requesterID, directMsg.ID, now).
		Scan(
			&senderMember.ChannelID,
			&senderMember.UserID,
			&senderMember.LastReadMessageID,
			&senderMember.JoinedAt,
		)
	if err != nil {
		return nil, mapPgError(err)
	}

	const addresseeMemberQuery = `
		INSERT INTO direct_channel_members (channel_id, user_id, last_read_message_id, joined_at)
		VALUES ($1, $2, NULL, $3)
		ON CONFLICT (channel_id, user_id) DO NOTHING
	`
	if _, err = tx.ExecContext(ctx, addresseeMemberQuery, directCh.ID, addresseeID, now); err != nil {
		return nil, mapPgError(err)
	}

	// Sidebar ordering sorts by channel activity, not channel creation.
	if _, err = tx.ExecContext(ctx, `UPDATE direct_channels SET updated_at = $1 WHERE id = $2`, now, directCh.ID); err != nil {
		return nil, err
	}

	if err = tx.Commit(); err != nil {
		return nil, err
	}

	return &Direct{
		DirectCh:             directCh,
		DirectMsg:            directMsg,
		DirectChannelMembers: senderMember,
	}, nil
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
	rows, err := s.db.QueryContext(ctx, `
		SELECT m.id, m.channel_id, m.author_id, m.content, COALESCE(m.reply_to, 0),
		       m.created_at, m.edited_at, m.deleted_at
		FROM direct_channels c
		JOIN direct_channel_members member ON member.channel_id = c.id AND member.user_id = $1
		JOIN direct_messages m ON m.channel_id = c.id
		WHERE c.dm_key = $2 AND m.deleted_at IS NULL AND ($3 = 0 OR m.id < $3)
		ORDER BY m.id DESC
		LIMIT $4`, viewerID, makeDMKey(int64(viewerID), int64(peerID)), beforeID, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []DirectMessage
	for rows.Next() {
		var m DirectMessage
		var editedAt, deletedAt sql.NullTime
		if err := rows.Scan(&m.ID, &m.ChannelID, &m.Author, &m.Content, &m.ReplyTo,
			&m.CreatedAt, &editedAt, &deletedAt); err != nil {
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
		UPDATE direct_messages SET content = $3, edited_at = now()
		WHERE id = $1 AND author_id = $2 AND deleted_at IS NULL
		RETURNING id, channel_id, author_id, content, COALESCE(reply_to, 0),
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
	if _, err := tx.ExecContext(ctx, `UPDATE direct_channels SET updated_at = now() WHERE id = $1`, m.ChannelID); err != nil {
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
		UPDATE direct_messages SET deleted_at = now()
		WHERE id = $1 AND author_id = $2 AND deleted_at IS NULL
		RETURNING id, channel_id`, messageID, userID).Scan(&id, &channelID)
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
	if _, err := tx.ExecContext(ctx, `UPDATE direct_channels SET updated_at = now() WHERE id = $1`, channelID); err != nil {
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
	var channelID, lastRead sql.NullInt64
	err := s.db.QueryRowContext(ctx, `
		SELECT c.id, m.last_read_message_id
		FROM direct_channels c
		JOIN direct_channel_members m ON m.channel_id = c.id AND m.user_id = $1
		WHERE c.dm_key = $2`, viewerID, makeDMKey(int64(viewerID), int64(peerID))).
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
			UPDATE direct_channel_members
			SET last_read_message_id = GREATEST(COALESCE(last_read_message_id, 0), $3)
			WHERE channel_id = $1 AND user_id = $2`, channelID.Int64, viewerID, upToID); err != nil {
			return state, err
		}
		state.LastReadMessageID = upToID
		state.Changed = true
	}
	err = s.db.QueryRowContext(ctx, `
		SELECT COUNT(*) FROM direct_messages
		WHERE channel_id = $1 AND author_id = $2 AND deleted_at IS NULL
		  AND id > $3`, channelID.Int64, peerID, state.LastReadMessageID).
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
		       (SELECT COUNT(*) FROM direct_messages unread
		        WHERE unread.channel_id = c.id AND unread.author_id = peer.user_id
		          AND unread.deleted_at IS NULL
		          AND unread.id > COALESCE(member.last_read_message_id, 0))
		FROM direct_channels c
		JOIN direct_channel_members member ON member.channel_id = c.id AND member.user_id = $1
		JOIN direct_channel_members peer ON peer.channel_id = c.id AND peer.user_id <> $1
		LEFT JOIN direct_messages last ON last.id = (
			SELECT id FROM direct_messages
			WHERE channel_id = c.id AND deleted_at IS NULL
			ORDER BY id DESC LIMIT 1
		)
		WHERE NOT EXISTS (
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
