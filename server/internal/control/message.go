package control

import (
	"encoding/json"

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
		CallOutcome: w.CallOutcome, CallSeconds: w.CallSeconds, E2EE: w.E2EE,
	}
}

func messageSummaries[T messages.WireMessage](list []T) []protocol.MessageSummary {
	out := make([]protocol.MessageSummary, len(list))
	for i, m := range list {
		out[i] = messageSummary(m.Wire())
	}
	return out
}

// maxPageBytes caps the message payload of a list ACK, well below the frame
// limit.
const maxPageBytes = 512 << 10

// pageOf returns the longest prefix of list, at most limit entries, whose JSON
// stays under maxPageBytes (but at least one), and whether entries were left.
func pageOf[T any](list []T, limit int) ([]T, bool) {
	used := 0
	for i, v := range list {
		b, _ := json.Marshal(v) // plain data: cannot fail
		used += len(b) + 1
		if i >= limit || (i > 0 && used > maxPageBytes) {
			return list[:i], true
		}
	}
	return list, false
}
