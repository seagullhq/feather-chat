package control

import (
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// messageSummary is the single store-to-wire mapping for both DMs and server
// text messages; their wire shape is the same.
func messageSummary(w messages.WireSummary) protocol.MessageSummary {
	return protocol.MessageSummary{
		ID: w.ID, ChannelID: protocol.ChannelID(w.ChannelID), AuthorID: w.AuthorID,
		Content: w.Content, ReplyTo: w.ReplyTo,
		CreatedAt: w.CreatedAt, EditedAt: w.EditedAt, DeletedAt: w.DeletedAt,
		CallOutcome: w.CallOutcome, CallSeconds: w.CallSeconds,
	}
}

func messageSummaries[T messages.WireMessage](list []T) []protocol.MessageSummary {
	out := make([]protocol.MessageSummary, len(list))
	for i, m := range list {
		out[i] = messageSummary(m.Wire())
	}
	return out
}
