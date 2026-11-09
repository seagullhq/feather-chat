package servers

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
)

type Channel struct {
	ID          int       `json:"id"`
	Name        string    `json:"name"`
	ChannelType int       `json:"channel_type"`
	ServerID    int       `json:"server_id"`
	CreatedBy   int       `json:"created_by"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

type ChannelType struct {
	ID       int    `json:"id"`
	TypeName string `json:"name"`
}

func (s *ServerStore) CreateChannel(ctx context.Context, channelName string, channelType int, serverID int, createdBy int) (*Channel, error) {
	now := time.Now().UTC()

	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()

	query := `
		INSERT INTO server_channels (name, channel_type, server_id, created_by, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6)
		RETURNING id
	`
	var channelID int
	err = tx.QueryRowContext(ctx, query, channelName, channelType, serverID, createdBy, now, now).Scan(&channelID)
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return nil, ErrTaken
		}
		return nil, err
	}

	if err := tx.Commit(); err != nil {
		return nil, err
	}

	channel := Channel{
		ID:          channelID,
		Name:        channelName,
		ChannelType: channelType,
		ServerID:    serverID,
		CreatedBy:   createdBy,
		CreatedAt:   now,
		UpdatedAt:   now,
	}
	return &channel, nil
}

func DeleteChannel(ctx context.Context, serverId int) {}

// ErrNotFound: the channel does not exist, is not a voice channel, or the
// user is not a member of its server. One error for all three so a probe
// cannot tell which channels exist.
var ErrNotFound = errors.New("channel not found")

// VoiceChannelForUser returns the voice channel only if the user owns or
// belongs to its server. This is the gate between a JOIN and a media room.
func (s *ServerStore) VoiceChannelForUser(ctx context.Context, channelID, userID int) (*Channel, error) {
	query := `
		SELECT c.id, c.name, COALESCE(c.channel_type, 0), c.server_id
		FROM server_channels c
		JOIN servers s ON s.id = c.server_id
		JOIN channel_types t ON t.id = c.channel_type
		WHERE c.id = $1
		  AND t.type_name = 'voice'
		  AND (
				s.owner_id = $2
				OR EXISTS (
					SELECT 1 FROM server_members m
					WHERE m.server_id = c.server_id AND m.user_id = $2
				)
		  )
	`
	var ch Channel
	err := s.db.QueryRowContext(ctx, query, channelID, userID).
		Scan(&ch.ID, &ch.Name, &ch.ChannelType, &ch.ServerID)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &ch, nil
}
