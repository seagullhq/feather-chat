package servers

import "context"

// ServerMember is one person of a server, for the members list.
type ServerMember struct {
	ID       int
	Username string
	Owner    bool
}

// ListMembers returns the people of a server, owner first. Same visibility
// rule as ListChannels: a non-member gets an empty list.
func (s *ServerStore) ListMembers(ctx context.Context, serverID, userID int) ([]ServerMember, error) {
	rows, err := s.db.QueryContext(ctx, `
		SELECT u.id, u.username, u.id = sv.owner_id
		FROM server_members m
		JOIN users u ON u.id = m.user_id
		JOIN servers sv ON sv.id = m.server_id
		WHERE m.server_id = $1
		  AND EXISTS (
				SELECT 1 FROM server_members me
				WHERE me.server_id = $1 AND me.user_id = $2
		  )
		ORDER BY (u.id = sv.owner_id) DESC, LOWER(u.username)
	`, serverID, userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	members := []ServerMember{}
	for rows.Next() {
		var m ServerMember
		if err := rows.Scan(&m.ID, &m.Username, &m.Owner); err != nil {
			return nil, err
		}
		members = append(members, m)
	}
	return members, rows.Err()
}
