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

var (
	ErrNotFound      = errors.New("DM channel not found")
	ErrSelfRequest   = errors.New("cannot send DM to yourself")
	ErrAlreadyExists = errors.New("DM already exists")
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

func makeDMKey(requesterID, addresseeID int64) string {
	a, b := min(requesterID, addresseeID), max(requesterID, addresseeID)
	return fmt.Sprintf("%d:%d", a, b)
}

func (s *ServerStore) SendDM(ctx context.Context, requesterID, addresseeID int, msg string) (*Direct, error) {
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

	const msgQuery = `
		INSERT INTO direct_messages (channel_id, author_id, content, created_at)
		VALUES ($1, $2, $3, $4)
		RETURNING id, channel_id, author_id, content, created_at
	`
	var directMsg DirectMessage
	err = tx.QueryRowContext(ctx, msgQuery, directCh.ID, requesterID, msg, now).
		Scan(
			&directMsg.ID,
			&directMsg.ChannelID,
			&directMsg.Author,
			&directMsg.Content,
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
		WHERE c.dm_key = $2 AND ($3 = 0 OR m.id < $3)
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
