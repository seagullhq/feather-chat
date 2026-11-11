package protocol

// Control is a TCP control message (§7): HELLO, JOIN, LEAVE, ACK, ROSTER.
type Control struct {
	Type            string                `json:"type"`
	Name            string                `json:"name,omitempty"`
	SSRC            uint32                `json:"ssrc,omitempty"`
	Room            string                `json:"room,omitempty"`
	ChannelID       int                   `json:"channelId,omitempty"`
	Members         []Member              `json:"members,omitempty"`
	Member          *Member               `json:"member,omitempty"`
	Muted           *bool                 `json:"muted,omitempty"`
	Deafened        *bool                 `json:"deafened,omitempty"`
	Username        string                `json:"username,omitempty"`
	ServerName      string                `json:"serverName,omitempty"`
	ServerID        int                   `json:"serverId,omitempty"`
	InviteCode      string                `json:"inviteCode,omitempty"`
	Servers         []ServerSummary       `json:"servers,omitempty"`
	Email           string                `json:"email,omitempty"`
	Identifier      string                `json:"identifier,omitempty"`
	Password        string                `json:"password,omitempty"`
	Error           string                `json:"error,omitempty"`
	ChannelName     string                `json:"channelName,omitempty"`
	ChannelType     string                `json:"channelType,omitempty"`
	ChannelTypeList []ChannelTypeSummary  `json:"channelTypes,omitempty"`
	Channels        []ChannelSummary      `json:"channels,omitempty"`
	ServerMembers   []ServerMemberSummary `json:"serverMembers,omitempty"`
}

// ServerMemberSummary is one person of a server, for the members list.
type ServerMemberSummary struct {
	ID       int    `json:"id"`
	Username string `json:"username"`
	Owner    bool   `json:"owner"`
}

// ServerSummary is the server identity the client needs to rebuild its sidebar.
type ServerSummary struct {
	ID         int    `json:"id"`
	Name       string `json:"name"`
	InviteCode string `json:"inviteCode"`
	// Owner is true when the asking user owns the server (may rename/delete it).
	Owner bool `json:"owner"`
}

// ChannelTypeSummary is one entry of the channel type catalogue.
type ChannelTypeSummary struct {
	ID       int    `json:"id"`
	TypeName string `json:"name"`
}

// ChannelSummary is the channel identity the client needs to rebuild a
// server's channel list.
type ChannelSummary struct {
	ID          int      `json:"id"`
	Name        string   `json:"name"`
	ChannelType int      `json:"channelType"`
	Members     []Member `json:"members,omitempty"`
}

// Member is one participant of a voice channel, as clients see it.
type Member struct {
	SSRC     uint32 `json:"ssrc"`
	Name     string `json:"name"`
	Muted    bool   `json:"muted"`
	Deafened bool   `json:"deafened"`
}
