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
