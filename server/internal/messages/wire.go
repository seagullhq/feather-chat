package messages

import "time"

// WireSummary is the part of a message shared by DMs and server channels.
// Both stored shapes convert to it so control needs one mapping to the wire.
type WireSummary struct {
	ID        int
	ChannelID int
	AuthorID  int
	Content   string
	ReplyTo   int
	CreatedAt time.Time
	EditedAt  *time.Time
	DeletedAt *time.Time
	// Call event fields; empty on text.
	CallOutcome string
	CallSeconds int
}

// WireMessage is implemented by every stored message type sent to a client.
type WireMessage interface {
	Wire() WireSummary
}

func (m DirectMessage) Wire() WireSummary {
	return WireSummary{
		ID: m.ID, ChannelID: m.ChannelID, AuthorID: m.Author,
		Content: m.Content, ReplyTo: m.ReplyTo,
		CreatedAt: m.CreatedAt, EditedAt: m.EditedAt, DeletedAt: m.DeletedAt,
		CallOutcome: m.CallOutcome, CallSeconds: m.CallSeconds,
	}
}

func (m MessageSummary) Wire() WireSummary {
	return WireSummary{
		ID: m.ID, ChannelID: int(m.ChannelID), AuthorID: m.AuthorID,
		Content: m.Content, ReplyTo: m.ReplyTo,
		CreatedAt: m.CreatedAt, EditedAt: m.EditedAt, DeletedAt: m.DeletedAt,
	}
}
