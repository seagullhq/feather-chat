package protocol

import (
	"time"

	"github.com/seagullhq/feather-chat/server/internal/e2ee"
)

// Distinct wire IDs. MessageID is conversations.id, ChannelID is a channel id
// and PageSize is a requested history page: keeping them separate from int
// makes a mix-up at a handler boundary a compile error.
type MessageID int
type ChannelID int64
type PageSize int

// Control is a TCP control message (§7): HELLO, JOIN, LEAVE, ACK, ROSTER.
type Control struct {
	Type              string                `json:"type"`
	Name              string                `json:"name,omitempty"`
	SSRC              uint32                `json:"ssrc,omitempty"`
	MediaKeyUp        string                `json:"mediaKeyUp,omitempty"`
	MediaKeyDown      string                `json:"mediaKeyDown,omitempty"`
	Room              string                `json:"room,omitempty"`
	ChannelID         int                   `json:"channelId,omitempty"`
	Members           []Member              `json:"members,omitempty"`
	Member            *Member               `json:"member,omitempty"`
	Muted             *bool                 `json:"muted,omitempty"`
	Deafened          *bool                 `json:"deafened,omitempty"`
	Username          string                `json:"username,omitempty"`
	ServerName        string                `json:"serverName,omitempty"`
	ServerID          int                   `json:"serverId,omitempty"`
	InviteCode        string                `json:"inviteCode,omitempty"`
	Servers           []ServerSummary       `json:"servers,omitempty"`
	Email             string                `json:"email,omitempty"`
	Identifier        string                `json:"identifier,omitempty"`
	Password          string                `json:"password,omitempty"`
	DeviceToken       string                `json:"deviceToken,omitempty"`
	Error             string                `json:"error,omitempty"`
	CallReason        string                `json:"callReason,omitempty"`
	ChannelName       string                `json:"channelName,omitempty"`
	ChannelType       string                `json:"channelType,omitempty"`
	ChannelTypeList   []ChannelTypeSummary  `json:"channelTypes,omitempty"`
	Channels          []ChannelSummary      `json:"channels,omitempty"`
	ServerMembers     []ServerMemberSummary `json:"serverMembers,omitempty"`
	UserID            int                   `json:"userId,omitempty"`
	TargetUserID      int                   `json:"targetUserId,omitempty"`
	User              int                   `json:"user,omitempty"`
	Query             string                `json:"query,omitempty"`
	Friends           []PersonSummary       `json:"friends,omitempty"`
	Users             []PersonSummary       `json:"users,omitempty"`
	DirectRecipientID string                `json:"directRecipientID,omitempty"`
	DirectContent     string                `json:"directContent,omitempty"`
	DirectReplyTo     int                   `json:"directReplyTo,omitempty"`
	DirectMessageID   int                   `json:"directMessageId,omitempty"`
	DMBeforeID        int                   `json:"dmBeforeId,omitempty"`
	DMLimit           int                   `json:"dmLimit,omitempty"`
	// UnreadCount and ServerHasMore are always present on their ACKs: the
	// client treats a missing field as an error, and 0/false are meaningful.
	UnreadCount         int                         `json:"unreadCount"`
	DirectInfo          *DirectMessageSummary       `json:"directInfo,omitempty"`
	DirectMessages      []DirectMessageSummary      `json:"directMessages,omitempty"`
	DirectConversations []DirectConversationSummary `json:"directConversations,omitempty"`
	ServerContent       string                      `json:"serverContent,omitempty"`
	ServerReplyTo       MessageID                   `json:"serverReplyTo,omitempty"`
	ServerMessageID     MessageID                   `json:"serverMessageId,omitempty"`
	ServerBeforeID      MessageID                   `json:"serverBeforeId,omitempty"`
	ServerLimit         PageSize                    `json:"serverLimit,omitempty"`
	ServerHasMore       bool                        `json:"serverHasMore"`
	ServerInfo          *ServerMessageSummary       `json:"serverInfo,omitempty"`
	ServerMessages      []ServerMessageSummary      `json:"serverMessages"`
	// Sharing mirrors Member.Sharing on DM_CALL_STATE, where there is no roster.
	Sharing bool `json:"sharing,omitempty"`

	// Account keys (docs/e2ee-text.md §3). AuthScheme is a pointer so that 0
	// is serialized.
	AuthScheme         *int              `json:"authScheme,omitempty"`
	Salt               []byte            `json:"salt,omitempty"`
	AuthKey            []byte            `json:"authKey,omitempty"`
	NewSalt            []byte            `json:"newSalt,omitempty"`
	NewAuthKey         []byte            `json:"newAuthKey,omitempty"`
	RecoveryAuthKey    []byte            `json:"recoveryAuthKey,omitempty"`
	NewRecoveryAuthKey []byte            `json:"newRecoveryAuthKey,omitempty"`
	AccountKeys        *e2ee.AccountKeys `json:"accountKeys,omitempty"`

	// Conversation keys and messages (§4, §5). The GET_CONVERSATION_KEYS ACK
	// is messages.ConversationKeys.
	ConversationID       int64          `json:"conversationId,omitempty"`
	BeforeEpoch          uint32         `json:"beforeEpoch,omitempty"`
	Epoch                uint32         `json:"epoch,omitempty"`
	KeyCommit            []byte         `json:"keyCommit,omitempty"`
	CreatorKeyID         int64          `json:"creatorKeyId,omitempty"`
	EpochSig             []byte         `json:"epochSig,omitempty"`
	Wraps                []e2ee.NewWrap `json:"wraps,omitempty"`
	SenderKeyID          int64          `json:"senderKeyId,omitempty"`
	Shares               []e2ee.NewWrap `json:"shares,omitempty"`
	SenderID             int            `json:"senderId,omitempty"`
	KeyID                int64          `json:"keyId,omitempty"`
	UserKeys             *e2ee.UserKeys `json:"userKeys,omitempty"`
	E2EE                 *e2ee.Message  `json:"e2ee,omitempty"`
	BeforeConversationID int            `json:"beforeConversationId,omitempty"`
	// HasMore is present on every list ACK of messages or conversations.
	HasMore *bool `json:"hasMore,omitempty"`
}

// DirectConversationSummary is one DM channel in a user's sidebar.
type DirectConversationSummary struct {
	ChannelID     int       `json:"channelId"`
	PeerID        int       `json:"peerId"`
	LastMessageID int       `json:"lastMessageId,omitempty"`
	Content       string    `json:"content,omitempty"`
	AuthorID      int       `json:"authorId,omitempty"`
	CreatedAt     time.Time `json:"createdAt"`
	UnreadCount   int       `json:"unreadCount,omitempty"`
	// E2EE is the preview of an encrypted last message; Content is then absent.
	E2EE *e2ee.Message `json:"e2ee,omitempty"`
}

// MessageSummary is one message as clients see it. Keep store types out of
// the wire format so IDs are exported and JSON keys stay camelCase. DMs and
// server text messages have the same shape; only the channelId domain differs.
type MessageSummary struct {
	ID        int        `json:"id"`
	ChannelID ChannelID  `json:"channelId"`
	AuthorID  int        `json:"authorId"`
	Content   string     `json:"content,omitempty"`
	ReplyTo   int        `json:"replyTo,omitempty"`
	CreatedAt time.Time  `json:"createdAt"`
	EditedAt  *time.Time `json:"editedAt,omitempty"`
	DeletedAt *time.Time `json:"deletedAt,omitempty"`
	// A finished 1:1 call: the outcome (ended, rejected, cancelled, missed,
	// offline) and, for ended, the talk time in seconds. Absent on text.
	CallOutcome string `json:"callOutcome,omitempty"`
	CallSeconds int    `json:"callSeconds,omitempty"`
	// E2EE is set on encrypted messages; Content is then absent.
	E2EE *e2ee.Message `json:"e2ee,omitempty"`
}

// Aliases keep each handler speaking its own domain without duplicating the
// struct or the mapping: the wire bytes are identical.
type DirectMessageSummary = MessageSummary
type ServerMessageSummary = MessageSummary

// PersonSummary is another user with the asking user's relation to them:
// "friend", "incoming", "outgoing" or "" (none).
type PersonSummary struct {
	ID       int    `json:"id"`
	Username string `json:"username"`
	Relation string `json:"relation"`
}

// ServerMemberSummary is one person of a server, for the members list.
type ServerMemberSummary struct {
	ID       int    `json:"id"`
	Username string `json:"username"`
	Owner    bool   `json:"owner"`
	Admin    bool   `json:"admin"`
	Me       bool   `json:"me"`
	Banned   bool   `json:"banned"`
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
	// UnreadCount is saturated at 100 ("99+" in the UI).
	UnreadCount int `json:"unreadCount,omitempty"`
}

// Member is one participant of a voice channel, as clients see it.
type Member struct {
	SSRC     uint32 `json:"ssrc"`
	Name     string `json:"name"`
	Muted    bool   `json:"muted"`
	Deafened bool   `json:"deafened"`
	Sharing  bool   `json:"sharing,omitempty"`
}
