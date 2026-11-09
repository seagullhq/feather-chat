package servers

import (
	"context"
)

// ListForUser returns every server the user belongs to, newest first. The
// owner is also a member (CreateServer inserts both rows), but the owner_id
// check keeps servers created before the membership table visible.
func (s *ServerStore) ListForUser(ctx context.Context, userID int) ([]Server, error) {
	query := `
		SELECT s.id, s.name, s.owner_id, s.invite_code, s.created_at, s.updated_at
		FROM servers s
		WHERE s.owner_id = $1
		   OR EXISTS (
				SELECT 1 FROM server_members m
				WHERE m.server_id = s.id AND m.user_id = $1
		   )
		ORDER BY s.created_at DESC, s.id DESC
	`

	rows, err := s.db.QueryContext(ctx, query, userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	servers := []Server{}
	for rows.Next() {
		var server Server
		if err := rows.Scan(
			&server.ID,
			&server.Name,
			&server.OwnerID,
			&server.InviteCode,
			&server.CreatedAt,
			&server.UpdatedAt,
		); err != nil {
			return nil, err
		}
		servers = append(servers, server)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return servers, nil
}

func (s *ServerStore) ListChannels(ctx context.Context, serverID, userID int) ([]Channel, error) {
	// Same visibility rule as ListForUser: owners and members only. A
	// non-member gets an empty list, so one server's channels never leak.
	query := `
		SELECT c.id, c.name, COALESCE(c.channel_type, 0), c.server_id,
		       COALESCE(c.created_by, 0), c.created_at, c.updated_at
		FROM server_channels c
		JOIN servers s ON s.id = c.server_id
		WHERE c.server_id = $1
		  AND (
				s.owner_id = $2
				OR EXISTS (
					SELECT 1 FROM server_members m
					WHERE m.server_id = c.server_id AND m.user_id = $2
				)
		  )
		ORDER BY c.created_at ASC, c.id ASC
	`

	rows, err := s.db.QueryContext(ctx, query, serverID, userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	channels := []Channel{}
	for rows.Next() {
		var channel Channel
		if err := rows.Scan(
			&channel.ID,
			&channel.Name,
			&channel.ChannelType,
			&channel.ServerID,
			&channel.CreatedBy,
			&channel.CreatedAt,
			&channel.UpdatedAt,
		); err != nil {
			return nil, err
		}
		channels = append(channels, channel)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return channels, nil
}

func (s *ServerStore) ListChannelType(ctx context.Context) ([]ChannelType, error) {
	query := `
		SELECT id, type_name FROM channel_types
		ORDER BY id
	`

	rows, err := s.db.QueryContext(ctx, query)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var channelTypes []ChannelType
	for rows.Next() {
		var channelType ChannelType
		if err := rows.Scan(&channelType.ID, &channelType.TypeName); err != nil {
			return nil, err
		}
		channelTypes = append(channelTypes, channelType)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	return channelTypes, nil
}
