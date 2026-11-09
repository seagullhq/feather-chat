package servers

import (
	"context"
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
