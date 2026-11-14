package servers

import "context"

// ServerMember is one person of a server, for the members list.
type ServerMember struct {
	ID       int
	Username string
	Owner    bool
	Admin    bool
	// Me marks the requesting user's own row.
	Me bool
	// Banned members are listed only to the owner and the admins, who can unban.
	Banned bool
}

// ListMembers returns the people of a server, owner first. Same visibility
// rule as ListChannels: a non-member or banned user gets an empty list. Banned
// members are included only when the requester is owner or admin.
func (s *ServerStore) ListMembers(ctx context.Context, serverID, userID int) ([]ServerMember, error) {
	seeBanned := canManage(ctx, s.db, serverID, userID) == nil
	rows, err := s.db.QueryContext(ctx, `
		SELECT u.id, u.username, u.id = sv.owner_id,
		       EXISTS (
					SELECT 1 FROM server_permissions sp
					JOIN permission_levels pl ON pl.id = sp.perm_level
					WHERE sp.server_id = m.server_id
					  AND sp.user_id = u.id
					  AND pl.permission_id = 'ADMIN'
		       ),
		       u.id = $2,
		       COALESCE(m.banned, false)
		FROM server_members m
		JOIN users u ON u.id = m.user_id
		JOIN servers sv ON sv.id = m.server_id
		WHERE m.server_id = $1
		  AND (COALESCE(m.banned, false) = false OR $3)
		  AND EXISTS (
				SELECT 1 FROM server_members me
				WHERE me.server_id = $1 AND me.user_id = $2
				  AND COALESCE(me.banned, false) = false
		  )
		ORDER BY (u.id = sv.owner_id) DESC, LOWER(u.username)
	`, serverID, userID, seeBanned)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	members := []ServerMember{}
	for rows.Next() {
		var m ServerMember
		if err := rows.Scan(&m.ID, &m.Username, &m.Owner, &m.Admin, &m.Me, &m.Banned); err != nil {
			return nil, err
		}
		members = append(members, m)
	}
	return members, rows.Err()
}

// MemberIDs returns the user ids of everyone who still belongs to a server.
func (s *ServerStore) MemberIDs(ctx context.Context, serverID int) ([]int, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT user_id FROM server_members
		WHERE server_id = $1 AND COALESCE(banned, false) = false`, serverID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var ids []int
	for rows.Next() {
		var id int
		if err := rows.Scan(&id); err != nil {
			return nil, err
		}
		ids = append(ids, id)
	}
	return ids, rows.Err()
}
