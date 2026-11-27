package control_test

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"net"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

type chatFixture struct {
	db            *sql.DB
	store         *accounts.Store
	serverStore   *servers.ServerStore
	msgStore      *messages.ServerStore
	ids           []int
	names         []string
	srv1, srv2    *servers.Server
	ch1, ch2, ch3 *servers.Channel
	voice         *servers.Channel
	threadType    int
	conns         []net.Conn
	ctx           context.Context
}

// setupChatFixture registers three users, two servers and a few channels.
// Callers close the connections; users are removed by t.Cleanup.
func setupChatFixture(t *testing.T) *chatFixture {
	t.Helper()
	store, db := testStore(t)
	const password = "integration-password-123"
	base := fmt.Sprintf("sm_%d", time.Now().UnixNano())
	names := []string{base, base + "_m", base + "_o"}
	ids := make([]int, len(names))

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := db.ExecContext(ctx, `DELETE FROM users WHERE LOWER(username) IN (LOWER($1), LOWER($2), LOWER($3))`,
			names[0], names[1], names[2]); err != nil {
			t.Logf("cleanup users: %v", err)
		}
	})

	// Covers the whole test: bcrypt under -race plus a 100k row seed takes
	// over 30s on a loaded machine. It only guards against a hung query.
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	t.Cleanup(cancel)
	for i, name := range names {
		if err := testRegister(ctx, store, name, name+"@example.test", password); err != nil {
			t.Fatalf("Register %s: %v", name, err)
		}
		u, err := store.Authenticate(ctx, name, testSecret(password))
		if err != nil {
			t.Fatalf("Authenticate %s: %v", name, err)
		}
		ids[i] = u.ID
	}

	ss := servers.NewStore(db)
	srv1, err := ss.CreateServer(ctx, ids[0], base+"a")
	if err != nil {
		t.Fatalf("CreateServer 1: %v", err)
	}
	srv2, err := ss.CreateServer(ctx, ids[0], base+"b")
	if err != nil {
		t.Fatalf("CreateServer 2: %v", err)
	}
	if _, err := ss.JoinServer(ctx, ids[1], srv1.InviteCode); err != nil {
		t.Fatalf("JoinServer 1: %v", err)
	}
	if _, err := ss.JoinServer(ctx, ids[1], srv2.InviteCode); err != nil {
		t.Fatalf("JoinServer 2: %v", err)
	}

	var threadType, voiceType int
	if err := db.QueryRowContext(ctx, `SELECT id FROM channel_types WHERE type_name='thread'`).Scan(&threadType); err != nil {
		t.Fatalf("thread type: %v", err)
	}
	if err := db.QueryRowContext(ctx, `SELECT id FROM channel_types WHERE type_name='voice'`).Scan(&voiceType); err != nil {
		t.Fatalf("voice type: %v", err)
	}
	mustChannel := func(name string, typ int, serverID int) *servers.Channel {
		ch, err := ss.CreateChannel(ctx, name, typ, serverID, ids[0])
		if err != nil {
			t.Fatalf("CreateChannel %s: %v", name, err)
		}
		return ch
	}
	ch1 := mustChannel("general", threadType, srv1.ID)
	ch2 := mustChannel("other", threadType, srv2.ID)
	ch3 := mustChannel("empty", threadType, srv1.ID)
	voice := mustChannel("voice-room", voiceType, srv1.ID)

	// Push the conversations sequence ahead of the channel sequence, so an
	// internal ID leaking onto the wire cannot pass by coincidence.
	if _, err := messages.NewStore(db).SendDM(ctx, ids[0], ids[1], 0, "seed dm"); err != nil {
		t.Fatalf("seed DM: %v", err)
	}

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	t.Cleanup(func() { _ = ltcp.Close(); _ = ludp.Close() })
	ms := messages.NewStore(db)
	serveTest(t, ltcp, ludp, store, ss, friends.NewStore(db), ms)

	addr := ltcp.Addr().String()
	conns := make([]net.Conn, len(names))
	for i, name := range names {
		conns[i] = dialControl(t, addr, uint32(0x1200+i), 0, name, name+"@example.test", password, false)
		t.Cleanup(func() { _ = conns[i].Close() })
	}

	return &chatFixture{
		db: db, store: store, serverStore: ss, msgStore: ms,
		ids: ids, names: names, srv1: srv1, srv2: srv2,
		ch1: ch1, ch2: ch2, ch3: ch3, voice: voice,
		threadType: threadType, conns: conns, ctx: ctx,
	}
}

func (f *chatFixture) conversationCount(t *testing.T, channelID int) int {
	t.Helper()
	var n int
	if err := f.db.QueryRowContext(f.ctx, `SELECT count(*) FROM conversations WHERE server_channel_id = $1`, channelID).Scan(&n); err != nil {
		t.Fatalf("conversation count: %v", err)
	}
	return n
}

func (f *chatFixture) messageCount(t *testing.T, channelID int) int {
	t.Helper()
	var n int
	if err := f.db.QueryRowContext(f.ctx, `
		SELECT count(*) FROM conversation_messages m
		JOIN conversations c ON c.id = m.conversation_id
		WHERE c.server_channel_id = $1`, channelID).Scan(&n); err != nil {
		t.Fatalf("message count: %v", err)
	}
	return n
}

func (f *chatFixture) expectError(t *testing.T, conn net.Conn, want string) {
	t.Helper()
	got := awaitControl(t, conn, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" })
	if got.Error != want {
		t.Fatalf("error = %q, want %q", got.Error, want)
	}
}

func TestIntegrationServerMessages(t *testing.T) {
	f := setupChatFixture(t)
	owner, member, outsider := f.conns[0], f.conns[1], f.conns[2]

	// A fresh thread costs no row until somebody writes to it.
	if n := f.conversationCount(t, f.ch1.ID); n != 0 {
		t.Fatalf("fresh channel has %d conversations, want 0", n)
	}

	// Validation happens before the store call and before rate limiting.
	for _, tc := range []struct {
		name string
		msg  protocol.Control
		want string
	}{
		{"whitespace", protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID}, "encrypted message required"},
		{"no server", protocol.Control{Type: "SEND_SERVER_MESSAGE", ChannelID: f.ch1.ID, ServerContent: "hi"}, "invalid server channel"},
	} {
		sendTest(t, owner, tc.msg)
		f.expectError(t, owner, tc.want)
	}

	// Outsiders, cross-server IDs and voice channels never create a row.
	for _, tc := range []struct {
		name string
		conn net.Conn
		msg  protocol.Control
	}{
		{"outsider", outsider, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID, ServerContent: "hi"}},
		{"wrong server", member, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv2.ID, ChannelID: f.ch1.ID, ServerContent: "hi"}},
		{"voice channel", owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.voice.ID, ServerContent: "hi"}},
	} {
		sendTest(t, tc.conn, tc.msg)
		f.expectError(t, tc.conn, "server channel not found")
	}
	if n := f.conversationCount(t, f.ch1.ID); n != 0 {
		t.Fatalf("denied sends created %d conversations, want 0", n)
	}

	// First real send: ACK and push agree, and channelId is the server channel.
	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID, ServerContent: "hello"}))
	ack := awaitControl(t, owner, "send ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	push := awaitControl(t, member, "SERVER_MESSAGE", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" && m.ServerInfo != nil })
	if ack.Type != "ACK" || ack.ServerInfo.ID != push.ServerInfo.ID {
		t.Fatalf("ACK %+v and push %+v disagree", ack, push)
	}
	if ack.ServerInfo.ChannelID != protocol.ChannelID(f.ch1.ID) || push.ServerInfo.ChannelID != protocol.ChannelID(f.ch1.ID) {
		t.Fatalf("channelId = %d/%d, want %d", ack.ServerInfo.ChannelID, push.ServerInfo.ChannelID, f.ch1.ID)
	}
	if ack.ServerID != f.srv1.ID || push.ServerID != f.srv1.ID {
		t.Fatalf("serverId = %d/%d, want %d", ack.ServerID, push.ServerID, f.srv1.ID)
	}
	firstID := ack.ServerInfo.ID
	var convID int
	if err := f.db.QueryRowContext(f.ctx, `SELECT id FROM conversations WHERE server_channel_id = $1`, f.ch1.ID).Scan(&convID); err != nil {
		t.Fatalf("conversation lookup: %v", err)
	}
	if convID == f.ch1.ID {
		t.Fatalf("internal conversation ID %d equals wire channelId", convID)
	}
	if n := f.conversationCount(t, f.ch1.ID); n != 1 {
		t.Fatalf("conversations after first message = %d, want 1", n)
	}

	// Second send reuses the same conversation.
	sendTest(t, member, seal(t, member, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID, ServerContent: "hi owner"}))
	second := awaitControl(t, member, "second ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, owner, "second push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" && m.ServerInfo != nil })
	secondID := second.ServerInfo.ID
	if n := f.conversationCount(t, f.ch1.ID); n != 1 {
		t.Fatalf("conversations after second message = %d, want 1", n)
	}

	// Sidebar badges exclude own messages and are populated by LIST_CHANNELS.
	sendTest(t, member, protocol.Control{Type: "LIST_CHANNELS", ServerID: f.srv1.ID})
	channels := awaitControl(t, member, "LIST_CHANNELS ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.Channels != nil })
	for _, ch := range channels.Channels {
		switch ch.ID {
		case f.ch1.ID:
			if ch.UnreadCount != 1 {
				t.Fatalf("ch1 unread = %d, want 1", ch.UnreadCount)
			}
		case f.ch3.ID:
			if ch.UnreadCount != 0 {
				t.Fatalf("empty channel unread = %d, want 0", ch.UnreadCount)
			}
		}
	}
	if n := f.conversationCount(t, f.ch3.ID); n != 0 {
		t.Fatalf("empty channel grew a conversation: %d", n)
	}

	// History is newest-first, typed as ACK, and advances the watermark only
	// on the first page: after fetching, the member has no unread messages.
	sendTest(t, member, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID})
	history := awaitControl(t, member, "history ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.ServerMessages != nil
	})
	if len(history.ServerMessages) != 2 {
		t.Fatalf("history length = %d, want 2", len(history.ServerMessages))
	}
	if history.ServerMessages[0].ID != secondID || history.ServerMessages[1].ID != firstID {
		t.Fatalf("history order = %+v, want [%d %d]", history.ServerMessages, secondID, firstID)
	}
	if history.ServerHasMore {
		t.Fatal("history of two messages reports hasMore")
	}
	if history.UnreadCount != 0 {
		t.Fatalf("history unread = %d, want 0 after mark-read", history.UnreadCount)
	}
	for _, m := range history.ServerMessages {
		if m.ChannelID != protocol.ChannelID(f.ch1.ID) {
			t.Fatalf("history channelId = %d, want %d", m.ChannelID, f.ch1.ID)
		}
	}

	// Bulk-insert messages from the owner: the member's badge counts them,
	// saturates at 100, and an explicit mark-read clears it monotonically.
	if _, err := f.db.ExecContext(f.ctx, `
		INSERT INTO conversation_messages (conversation_id, author_id, content)
		SELECT $1, $2, 'bulk ' || g FROM generate_series(1, 250) g`, convID, f.ids[0]); err != nil {
		t.Fatalf("bulk insert: %v", err)
	}
	sendTest(t, member, protocol.Control{Type: "LIST_CHANNELS", ServerID: f.srv1.ID})
	channels = awaitControl(t, member, "saturated badge", func(m protocol.Control) bool { return m.Type == "ACK" && m.Channels != nil })
	for _, ch := range channels.Channels {
		if ch.ID == f.ch1.ID && ch.UnreadCount != 100 {
			t.Fatalf("saturated unread = %d, want 100", ch.UnreadCount)
		}
	}
	var newest int
	if err := f.db.QueryRowContext(f.ctx, `SELECT max(id) FROM conversation_messages WHERE conversation_id = $1`, convID).Scan(&newest); err != nil {
		t.Fatalf("max id: %v", err)
	}
	sendTest(t, member, protocol.Control{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(newest)})
	marked := awaitControl(t, member, "mark read ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessageID != 0 })
	if marked.ServerMessageID != protocol.MessageID(newest) || marked.UnreadCount != 0 {
		t.Fatalf("mark ACK = %+v, want watermark %d and unread 0", marked, newest)
	}
	sendTest(t, member, protocol.Control{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(firstID)})
	older := awaitControl(t, member, "older mark ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessageID != 0 })
	if older.ServerMessageID != protocol.MessageID(newest) {
		t.Fatalf("watermark regressed to %d, want %d", older.ServerMessageID, newest)
	}

	// An older page (beforeId > 0) must not move the watermark either.
	sendTest(t, member, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID, ServerBeforeID: protocol.MessageID(newest), ServerLimit: 5})
	oldPage := awaitControl(t, member, "old page ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessages != nil })
	if !oldPage.ServerHasMore || oldPage.UnreadCount != 0 {
		t.Fatalf("old page hasMore=%v unread=%d, want true/0", oldPage.ServerHasMore, oldPage.UnreadCount)
	}
	sendTest(t, member, protocol.Control{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(firstID)})
	still := awaitControl(t, member, "watermark check", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessageID != 0 })
	if still.ServerMessageID != protocol.MessageID(newest) {
		t.Fatalf("watermark after old page = %d, want %d", still.ServerMessageID, newest)
	}

	// Only the author may edit or delete. Cross-channel IDs and other users'
	// messages produce the same generic error.
	sendTest(t, member, seal(t, member, protocol.Control{Type: "UPDATE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(firstID), ServerContent: "stolen"}))
	f.expectError(t, member, "server message not found")
	var content string
	if err := f.db.QueryRowContext(f.ctx, `SELECT convert_from(substring(e2ee_box FROM 1 FOR 5), 'UTF8') FROM conversation_messages WHERE id = $1`, firstID).Scan(&content); err != nil || content != "hello" {
		t.Fatalf("message after denied edit = %q, err %v", content, err)
	}
	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "UPDATE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(firstID), ServerContent: "edited"}))
	edited := awaitControl(t, owner, "edit ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, member, "edit push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE_EDITED" && m.ServerInfo != nil })
	if textOf(edited.ServerInfo.Content, edited.ServerInfo.E2EE) != "edited" || edited.ServerInfo.EditedAt == nil {
		t.Fatalf("edit ACK = %+v", edited.ServerInfo)
	}
	sendTest(t, member, protocol.Control{Type: "DELETE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(firstID)})
	f.expectError(t, member, "server message not found")
	sendTest(t, owner, protocol.Control{Type: "DELETE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(firstID)})
	deleted := awaitControl(t, owner, "delete ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessageID != 0 })
	awaitControl(t, member, "delete push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE_DELETED" })
	if deleted.ServerMessageID != protocol.MessageID(firstID) {
		t.Fatalf("delete ACK id = %d, want %d", deleted.ServerMessageID, firstID)
	}
	var tombstone string
	var deletedAt sql.NullTime
	if err := f.db.QueryRowContext(f.ctx, `SELECT content, deleted_at FROM conversation_messages WHERE id = $1`, firstID).Scan(&tombstone, &deletedAt); err != nil {
		t.Fatalf("tombstone lookup: %v", err)
	}
	if tombstone != "" || !deletedAt.Valid {
		t.Fatalf("tombstone = %q, deletedAt = %+v", tombstone, deletedAt)
	}

	// Marking a message of another channel is rejected.
	sendTest(t, member, protocol.Control{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch2.ID, ServerMessageID: protocol.MessageID(secondID)})
	f.expectError(t, member, "server message not found")

	// Reads are gated too, and an outsider cannot create a conversation.
	sendTest(t, outsider, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID})
	f.expectError(t, outsider, "server channel not found")
	sendTest(t, outsider, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.voice.ID})
	f.expectError(t, outsider, "server channel not found")
	if n := f.conversationCount(t, f.ch3.ID); n != 0 {
		t.Fatalf("outsider reads created %d conversations", n)
	}

	// DB-level invariants: shape is immutable and the CHECKs reject hybrids.
	for _, stmt := range []string{
		`INSERT INTO conversations (kind, server_channel_id, user_low, user_high) VALUES ('server', ` + strconv.Itoa(f.ch3.ID) + `, 1, 2)`,
		`INSERT INTO conversations (kind, server_channel_id, name) VALUES ('server', ` + strconv.Itoa(f.ch3.ID) + `, 'bad')`,
		`UPDATE conversations SET kind = 'group' WHERE id = ` + strconv.Itoa(convID),
		`UPDATE conversations SET server_channel_id = ` + strconv.Itoa(f.ch2.ID) + ` WHERE id = ` + strconv.Itoa(convID),
	} {
		if _, err := f.db.ExecContext(f.ctx, stmt); err == nil {
			t.Fatalf("statement should have been rejected: %s", stmt)
		}
	}
	if _, err := f.db.ExecContext(f.ctx, `UPDATE conversations SET updated_at = now() WHERE id = $1`, convID); err != nil {
		t.Fatalf("updated_at update should still work: %v", err)
	}

	// Page byte budget: 100 long messages must be split, not sent as one huge
	// frame, and keyset pagination must return every message exactly once.
	long := strings.Repeat("界", 4000) // 12 KB each in UTF-8
	if _, err := f.db.ExecContext(f.ctx, `
		INSERT INTO conversation_messages (conversation_id, author_id, content)
		SELECT $1, $2, $3 FROM generate_series(1, 100)`, convID, f.ids[0], long); err != nil {
		t.Fatalf("long message insert: %v", err)
	}
	seen := make(map[int]bool)
	before := 0
	maxFrame := 0
	for page := 0; page < 100; page++ {
		sendTest(t, member, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID,
			ServerBeforeID: protocol.MessageID(before), ServerLimit: 100})
		member.SetReadDeadline(time.Now().Add(10 * time.Second))
		payload, err := protocol.ReadFrame(member)
		member.SetReadDeadline(time.Time{})
		if err != nil {
			t.Fatalf("paged history frame: %v", err)
		}
		// The client rejects frames >= 1 MiB with "connection lost".
		if len(payload) >= 1<<20 {
			t.Fatalf("page %d frame is %d bytes, at/above the 1 MiB protocol cap", page, len(payload))
		}
		if len(payload) > maxFrame {
			maxFrame = len(payload)
		}
		var resp protocol.Control
		if err := json.Unmarshal(payload, &resp); err != nil {
			t.Fatalf("paged history JSON: %v", err)
		}
		if resp.Type == "ERROR" {
			t.Fatalf("paged history error: %s", resp.Error)
		}
		if resp.Type != "ACK" {
			t.Fatalf("paged history type = %q, want ACK", resp.Type)
		}
		if len(resp.ServerMessages) == 0 {
			t.Fatal("empty page before hasMore became false")
		}
		for _, msg := range resp.ServerMessages {
			if seen[msg.ID] {
				t.Fatalf("duplicate message %d in pagination", msg.ID)
			}
			seen[msg.ID] = true
			before = msg.ID
		}
		if !resp.ServerHasMore {
			break
		}
		if page == 99 {
			t.Fatal("pagination did not terminate")
		}
	}
	if len(seen) < 100 {
		t.Fatalf("pagination returned %d messages, want at least 100 long ones", len(seen))
	}
	// Mirrors control.maxPageBytes (512 KiB) plus the other ACK fields.
	if maxFrame > (512<<10)+1024 || maxFrame < 256<<10 {
		t.Fatalf("largest frame is %d bytes, want 256-513 KiB", maxFrame)
	}

	// Kick removes the read-pointer row through the cleanup trigger.
	if _, err := f.db.ExecContext(f.ctx, `INSERT INTO conversation_members (conversation_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING`, convID, f.ids[1]); err != nil {
		t.Fatalf("membership insert: %v", err)
	}
	if err := f.serverStore.KickUser(f.ctx, f.ids[0], f.srv1.ID, f.ids[1]); err != nil {
		t.Fatalf("KickUser: %v", err)
	}
	var memberships int
	if err := f.db.QueryRowContext(f.ctx, `SELECT count(*) FROM conversation_members WHERE conversation_id = $1 AND user_id = $2`, convID, f.ids[1]).Scan(&memberships); err != nil {
		t.Fatalf("membership count: %v", err)
	}
	if memberships != 0 {
		t.Fatalf("kicked member still has %d read-pointer rows", memberships)
	}
}

// awaitAckOrError accepts both outcomes: awaitControl treats every ERROR as
// fatal unless the caller is explicitly waiting for one.
func awaitAckOrError(t *testing.T, c net.Conn) protocol.Control {
	t.Helper()
	c.SetReadDeadline(time.Now().Add(10 * time.Second))
	defer c.SetReadDeadline(time.Time{})
	for {
		payload, err := protocol.ReadFrame(c)
		if err != nil {
			t.Fatalf("awaiting burst response: %v", err)
		}
		var m protocol.Control
		if json.Unmarshal(payload, &m) != nil {
			continue
		}
		if m.Type == "ACK" || m.Type == "ERROR" {
			return m
		}
	}
}

func TestIntegrationServerMessageRateLimit(t *testing.T) {
	f := setupChatFixture(t)
	sender := f.conns[1] // member of srv1, fresh limiter bucket

	// Send the whole burst before reading: waiting for each ACK would let the
	// 1 msg/s bucket refill and hide the limit.
	const burst = 10
	for i := 0; i < burst; i++ {
		sendTest(t, sender, seal(t, sender, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
			ChannelID: f.ch1.ID, ServerContent: fmt.Sprintf("burst %d", i)}))
	}
	var ok, rejected int
	for i := 0; i < burst; i++ {
		m := awaitAckOrError(t, sender)
		if m.Type == "ERROR" {
			if m.Error != "server message rate limit exceeded" {
				t.Fatalf("rate limit error = %q", m.Error)
			}
			rejected++
			continue
		}
		ok++
	}
	if rejected == 0 || ok >= burst {
		t.Fatalf("burst: %d accepted, %d rejected, want some rejections", ok, rejected)
	}
	count := f.messageCount(t, f.ch1.ID)
	if count != ok {
		t.Fatalf("stored %d messages, want %d (one per ACK)", count, ok)
	}

	// One refill makes one more token available: the limiter must recover
	// without recreating the handler or the test connection.
	time.Sleep(1100 * time.Millisecond)
	sendTest(t, sender, seal(t, sender, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "after refill"}))
	refilled := awaitAckOrError(t, sender)
	if refilled.Type != "ACK" || refilled.ServerInfo == nil {
		t.Fatalf("message after refill = %+v, want ACK", refilled)
	}
}

// readServerAck is the goroutine-safe counterpart of awaitControl: it never
// calls t.Fatal, so concurrent sends report errors through a channel instead.
func readServerAck(c net.Conn) (protocol.Control, error) {
	if err := c.SetReadDeadline(time.Now().Add(15 * time.Second)); err != nil {
		return protocol.Control{}, err
	}
	defer c.SetReadDeadline(time.Time{})
	for {
		payload, err := protocol.ReadFrame(c)
		if err != nil {
			return protocol.Control{}, err
		}
		var m protocol.Control
		if json.Unmarshal(payload, &m) != nil {
			continue
		}
		if m.Type == "ERROR" {
			return m, fmt.Errorf("server error %q", m.Error)
		}
		if m.Type == "ACK" && m.ServerInfo != nil {
			return m, nil
		}
	}
}

func TestConcurrentServerFirstMessages(t *testing.T) {
	f := setupChatFixture(t)

	type result struct {
		id  int
		err error
	}
	results := make(chan result, 2)
	start := make(chan struct{})
	for i, conn := range []net.Conn{f.conns[0], f.conns[1]} {
		msg := seal(t, conn, protocol.Control{
			Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
			ChannelID: f.ch1.ID, ServerContent: fmt.Sprintf("concurrent %d", i),
		})
		go func(conn net.Conn) {
			<-start
			payload, err := json.Marshal(msg)
			if err == nil {
				err = protocol.WriteFrame(conn, payload)
			}
			if err != nil {
				results <- result{err: err}
				return
			}
			ack, err := readServerAck(conn)
			results <- result{id: ack.ServerInfo.ID, err: err}
		}(conn)
	}
	close(start)

	ids := make(map[int]bool)
	for i := 0; i < 2; i++ {
		r := <-results
		if r.err != nil {
			t.Fatalf("concurrent send %d: %v", i, r.err)
		}
		if ids[r.id] {
			t.Fatalf("two sends returned message id %d", r.id)
		}
		ids[r.id] = true
	}

	if n := f.conversationCount(t, f.ch1.ID); n != 1 {
		t.Fatalf("conversations after concurrent first sends = %d, want 1", n)
	}
	count := f.messageCount(t, f.ch1.ID)
	if count != 2 {
		t.Fatalf("messages after concurrent first sends = %d, want 2", count)
	}
}

func TestDeletedServerMessageAuthor(t *testing.T) {
	f := setupChatFixture(t)
	member := f.conns[1]

	sendTest(t, member, seal(t, member, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "from a user who will be deleted"}))
	ack := awaitControl(t, member, "send ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	messageID := ack.ServerInfo.ID
	awaitControl(t, f.conns[0], "push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })

	if _, err := f.db.ExecContext(f.ctx, `DELETE FROM users WHERE id = $1`, f.ids[1]); err != nil {
		t.Fatalf("delete author: %v", err)
	}
	var author sql.NullInt64
	if err := f.db.QueryRowContext(f.ctx, `SELECT author_id FROM conversation_messages WHERE id = $1`, messageID).Scan(&author); err != nil {
		t.Fatalf("author lookup: %v", err)
	}
	if author.Valid {
		t.Fatalf("author_id = %d after user deletion, want NULL", author.Int64)
	}

	// A NULL author must not break the int scan, and must count as unread for
	// everybody else (IS DISTINCT FROM, not <> with a magic zero).
	sendTest(t, f.conns[0], protocol.Control{Type: "LIST_CHANNELS", ServerID: f.srv1.ID})
	channels := awaitControl(t, f.conns[0], "badges", func(m protocol.Control) bool { return m.Type == "ACK" && m.Channels != nil })
	for _, ch := range channels.Channels {
		if ch.ID == f.ch1.ID && ch.UnreadCount != 1 {
			t.Fatalf("unread after author deletion = %d, want 1", ch.UnreadCount)
		}
	}
	sendTest(t, f.conns[0], protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID})
	history := awaitControl(t, f.conns[0], "history", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessages != nil })
	if len(history.ServerMessages) != 1 || history.ServerMessages[0].AuthorID != f.ids[1] {
		t.Fatalf("history with deleted author = %+v", history.ServerMessages)
	}
}

func TestServerChannelTypeTransitions(t *testing.T) {
	f := setupChatFixture(t)
	owner := f.conns[0]
	var voiceType int
	if err := f.db.QueryRowContext(f.ctx, `SELECT id FROM channel_types WHERE type_name = 'voice'`).Scan(&voiceType); err != nil {
		t.Fatalf("voice type: %v", err)
	}

	setType := func(typeID int) {
		if _, err := f.db.ExecContext(f.ctx, `UPDATE server_channels SET channel_type = $1 WHERE id = $2`, typeID, f.voice.ID); err != nil {
			t.Fatalf("set channel type: %v", err)
		}
	}

	// voice -> thread: the conversation is born at the first message.
	setType(f.threadType)
	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.voice.ID, ServerContent: "now a text channel"}))
	awaitControl(t, owner, "first ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, f.conns[1], "push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })
	if n := f.conversationCount(t, f.voice.ID); n != 1 {
		t.Fatalf("conversations after voice -> thread = %d, want 1", n)
	}

	// thread -> voice: the row and its history stay, but the channel is no
	// longer a readable server text channel.
	setType(voiceType)
	sendTest(t, owner, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.voice.ID})
	f.expectError(t, owner, "server channel not found")
	if n := f.conversationCount(t, f.voice.ID); n != 1 {
		t.Fatalf("conversation lost on thread -> voice: %d", n)
	}

	// voice -> thread again: the same conversation is reused, no duplicate.
	setType(f.threadType)
	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.voice.ID, ServerContent: "back to text"}))
	awaitControl(t, owner, "second ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, f.conns[1], "second push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })
	if n := f.conversationCount(t, f.voice.ID); n != 1 {
		t.Fatalf("conversations after second transition = %d, want 1", n)
	}
	sendTest(t, owner, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.voice.ID})
	history := awaitControl(t, owner, "history", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessages != nil })
	if len(history.ServerMessages) != 2 {
		t.Fatalf("history after transitions has %d messages, want 2", len(history.ServerMessages))
	}
}

func TestServerChannelDeleteCascade(t *testing.T) {
	f := setupChatFixture(t)
	owner, member := f.conns[0], f.conns[1]

	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "parent"}))
	parent := awaitControl(t, owner, "parent ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, member, "parent push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })

	sendTest(t, member, seal(t, member, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "reply", ServerReplyTo: protocol.MessageID(parent.ServerInfo.ID)}))
	reply := awaitControl(t, member, "reply ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, owner, "reply push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })

	// A read pointer on the channel must not block the cascade either.
	sendTest(t, member, protocol.Control{Type: "MARK_SERVER_MESSAGE_READ",
		ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(reply.ServerInfo.ID)})
	awaitControl(t, member, "mark ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessageID != 0 })

	if _, err := f.db.ExecContext(f.ctx, `DELETE FROM server_channels WHERE id = $1`, f.ch1.ID); err != nil {
		t.Fatalf("delete channel: %v", err)
	}
	for _, tc := range []struct {
		name  string
		query string
		args  []any
		want  int
	}{
		{"conversations", `SELECT count(*) FROM conversations WHERE server_channel_id = $1`, []any{f.ch1.ID}, 0},
		{"messages", `SELECT count(*) FROM conversation_messages m JOIN conversations c ON c.id = m.conversation_id WHERE c.server_channel_id = $1`, []any{f.ch1.ID}, 0},
		{"members", `SELECT count(*) FROM conversation_members cm JOIN conversations c ON c.id = cm.conversation_id WHERE c.server_channel_id = $1`, []any{f.ch1.ID}, 0},
		{"dm conversations", `
			SELECT count(*) FROM conversations
			WHERE kind = 'direct'
			  AND user_low = LEAST($1::bigint, $2::bigint)
			  AND user_high = GREATEST($1::bigint, $2::bigint)`,
			[]any{f.ids[0], f.ids[1]}, 1},
		{"other channels", `SELECT count(*) FROM server_channels WHERE server_id = $1`, []any{f.srv1.ID}, 2},
	} {
		var got int
		if err := f.db.QueryRowContext(f.ctx, tc.query, tc.args...).Scan(&got); err != nil {
			t.Fatalf("count %s: %v", tc.name, err)
		}
		if got != tc.want {
			t.Fatalf("%s after delete = %d, want %d", tc.name, got, tc.want)
		}
	}
}

func TestBannedServerMemberCannotReadOrWrite(t *testing.T) {
	f := setupChatFixture(t)
	owner, member := f.conns[0], f.conns[1]

	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "before the ban"}))
	first := awaitControl(t, owner, "send ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, member, "push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })

	// Give the member a read pointer, then ban: the cleanup trigger must
	// remove it together with the membership.
	sendTest(t, member, protocol.Control{Type: "MARK_SERVER_MESSAGE_READ",
		ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(first.ServerInfo.ID)})
	awaitControl(t, member, "mark ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerMessageID != 0 })
	if err := f.serverStore.BanUser(f.ctx, f.ids[0], f.srv1.ID, f.ids[1]); err != nil {
		t.Fatalf("BanUser: %v", err)
	}
	var pointers int
	if err := f.db.QueryRowContext(f.ctx, `
		SELECT count(*) FROM conversation_members cm
		JOIN conversations c ON c.id = cm.conversation_id
		WHERE c.server_channel_id = $1 AND cm.user_id = $2`, f.ch1.ID, f.ids[1]).Scan(&pointers); err != nil {
		t.Fatalf("read pointer count: %v", err)
	}
	if pointers != 0 {
		t.Fatalf("banned member still has %d read pointers", pointers)
	}

	for _, msg := range []protocol.Control{
		{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID, ServerContent: "after ban"},
		{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID},
		{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(first.ServerInfo.ID)},
		{Type: "UPDATE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(first.ServerInfo.ID), ServerContent: "stolen"},
		{Type: "DELETE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(first.ServerInfo.ID)},
	} {
		sendTest(t, member, msg)
		got := awaitControl(t, member, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" })
		if got.Error != "server channel not found" && got.Error != "server message not found" {
			t.Fatalf("%s after ban = %q", msg.Type, got.Error)
		}
	}
	// No push may have been emitted for the rejected send.
	member.SetReadDeadline(time.Now().Add(150 * time.Millisecond))
	_, err := protocol.ReadFrame(member)
	member.SetReadDeadline(time.Time{})
	if err == nil {
		t.Fatal("banned member received an unexpected frame after rejected sends")
	}

	count := f.messageCount(t, f.ch1.ID)
	if count != 1 {
		t.Fatalf("messages after banned writes = %d, want 1", count)
	}
}

func explainPlan(t *testing.T, f *chatFixture, query string, args ...any) string {
	t.Helper()
	tx, err := f.db.BeginTx(f.ctx, nil)
	if err != nil {
		t.Fatalf("begin EXPLAIN: %v", err)
	}
	defer tx.Rollback()
	// No planner knobs: the plan must be the one production would choose.
	// The 100k-row fixture makes the ordered keyset plan the cheapest one.
	rows, err := tx.QueryContext(f.ctx, query, args...)
	if err != nil {
		t.Fatalf("EXPLAIN: %v", err)
	}
	defer rows.Close()
	var lines []string
	for rows.Next() {
		var line string
		if err := rows.Scan(&line); err != nil {
			t.Fatalf("scan EXPLAIN: %v", err)
		}
		lines = append(lines, line)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("EXPLAIN rows: %v", err)
	}
	return strings.Join(lines, "\n")
}

func TestServerMessageACKFieldsAlwaysPresent(t *testing.T) {
	f := setupChatFixture(t)
	owner, member := f.conns[0], f.conns[1]

	readACK := func(c net.Conn) []byte {
		t.Helper()
		c.SetReadDeadline(time.Now().Add(10 * time.Second))
		defer c.SetReadDeadline(time.Time{})
		payload, err := protocol.ReadFrame(c)
		if err != nil {
			t.Fatalf("read ACK: %v", err)
		}
		var m protocol.Control
		if err := json.Unmarshal(payload, &m); err != nil || m.Type != "ACK" {
			t.Fatalf("want ACK, got %s (%v)", payload, err)
		}
		return payload
	}
	assertFields := func(payload []byte, fields ...string) {
		t.Helper()
		for _, field := range fields {
			if !bytes.Contains(payload, []byte(field)) {
				t.Fatalf("ACK is missing %s: %s", field, payload)
			}
		}
	}

	// Empty history: both zero values are meaningful and must be serialized.
	sendTest(t, member, protocol.Control{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch3.ID})
	assertFields(readACK(member), `"serverHasMore":false`, `"unreadCount":0`, `"serverMessages":[]`)

	// Mark-read ACK: unreadCount is 0 after the watermark advances.
	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch3.ID, ServerContent: "mark me"}))
	sent := awaitControl(t, owner, "send ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, member, "send push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })

	sendTest(t, member, protocol.Control{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch3.ID,
		ServerMessageID: protocol.MessageID(sent.ServerInfo.ID)})
	assertFields(readACK(member), `"unreadCount":0`)
}

func TestServerMessageQueryPlans(t *testing.T) {
	f := setupChatFixture(t)
	owner, member := f.conns[0], f.conns[1]

	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "plan seed"}))
	awaitControl(t, owner, "send ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, member, "push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })

	// A realistic dataset and page size let the planner choose the ordered
	// keyset walk on its own, with no enable_* knobs distorting the plan.
	var convID int
	if err := f.db.QueryRowContext(f.ctx, `
		SELECT id FROM conversations WHERE server_channel_id = $1`, f.ch1.ID).Scan(&convID); err != nil {
		t.Fatalf("conversation lookup: %v", err)
	}
	if _, err := f.db.ExecContext(f.ctx, `
		INSERT INTO conversation_messages (conversation_id, author_id, content)
		SELECT $1, $2, 'plan seed ' || g FROM generate_series(1, 100000) g`, convID, f.ids[0]); err != nil {
		t.Fatalf("bulk seed: %v", err)
	}
	if _, err := f.db.ExecContext(f.ctx, `ANALYZE conversation_messages`); err != nil {
		t.Fatalf("analyze: %v", err)
	}

	// Same shape as ListChannelMessages: keyset pagination must walk an index
	// backwards (no Sort), regardless of which ordered index the planner picks.
	historyPlan := explainPlan(t, f, `
		EXPLAIN
		SELECT m.id, $2::bigint, $3::bigint, COALESCE(m.author_id, 0),
		       m.content, COALESCE(m.reply_to, 0), m.created_at, m.edited_at, m.deleted_at
		FROM conversation_messages m
		WHERE m.conversation_id = $1
		  AND m.deleted_at IS NULL
		  AND ($4 = 0 OR m.id < $4)
		ORDER BY m.id DESC
		LIMIT $5`, convID, f.ch1.ID, f.srv1.ID, 0, 51)
	if strings.Contains(historyPlan, "Sort") {
		t.Fatalf("history plan has a Sort node:\n%s", historyPlan)
	}
	if !strings.Contains(historyPlan, "Index Scan Backward") {
		t.Fatalf("history plan does not walk an ordered index backwards:\n%s", historyPlan)
	}

	// Same shape as UnreadCountsForUser: the table function must inline and
	// the LATERAL LIMIT must cap the scan at 100 rows per channel.
	badgePlan := explainPlan(t, f, `
		EXPLAIN
		SELECT v.server_channel_id, u.unread
		FROM server_chat_channels v
		JOIN readable_server_channels($2) r ON r.server_channel_id = v.server_channel_id
		LEFT JOIN conversation_members cm
		  ON cm.conversation_id = v.conversation_id AND cm.user_id = $2
		CROSS JOIN LATERAL (
			SELECT count(*) AS unread FROM (
				SELECT 1 FROM conversation_messages m
				WHERE m.conversation_id = v.conversation_id
				  AND m.id > COALESCE(cm.last_read_message_id, 0)
				  AND m.deleted_at IS NULL
				  AND m.author_id IS DISTINCT FROM $2
				LIMIT 100
			) capped
		) u
		WHERE v.server_id = $1`, f.srv1.ID, f.ids[0])
	if strings.Contains(badgePlan, "Function Scan on readable_server_channels") {
		t.Fatalf("readable_server_channels was not inlined:\n%s", badgePlan)
	}
	if !strings.Contains(badgePlan, "Limit") {
		t.Fatalf("badge plan does not cap the lateral scan:\n%s", badgePlan)
	}
}

// TestServerChatAuthorizationBoundaries covers the BOLA and ID-enumeration
// cases that need a fresh, unauthorised session: every guessed ID must return
// the same generic error and leave state and other recipients untouched.
func TestServerChatAuthorizationBoundaries(t *testing.T) {
	f := setupChatFixture(t)
	owner, member, outsider := f.conns[0], f.conns[1], f.conns[2]

	sendTest(t, owner, seal(t, owner, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "boundary seed"}))
	seed := awaitControl(t, owner, "seed ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, member, "seed push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })
	messageID := seed.ServerInfo.ID
	if messageID <= 1 {
		t.Fatalf("seed message id = %d, need room for ±1 probes", messageID)
	}

	// The member belongs to both servers, but a channel is bound to its server:
	// neither direction of a mismatched (serverId, channelId) pair is accepted.
	for _, tc := range []struct {
		name      string
		serverID  int
		channelID int
	}{
		{"server A with channel B", f.srv1.ID, f.ch2.ID},
		{"server B with channel A", f.srv2.ID, f.ch1.ID},
	} {
		sendTest(t, member, seal(t, member, protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: tc.serverID,
			ChannelID: tc.channelID, ServerContent: "cross-server"}))
		f.expectError(t, member, "server channel not found")
	}

	// A server conversation must never leak into the DM sidebar.
	var serverConvID int
	if err := f.db.QueryRowContext(f.ctx, `SELECT id FROM conversations WHERE server_channel_id = $1`, f.ch1.ID).Scan(&serverConvID); err != nil {
		t.Fatalf("server conversation lookup: %v", err)
	}
	sendTest(t, member, protocol.Control{Type: "LIST_DIRECT_CONVERSATIONS"})
	sidebar := awaitControl(t, member, "DM sidebar", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.DirectConversations != nil
	})
	if len(sidebar.DirectConversations) != 1 {
		t.Fatalf("DM sidebar has %d entries, want the seeded DM only", len(sidebar.DirectConversations))
	}
	for _, conv := range sidebar.DirectConversations {
		if conv.ChannelID == serverConvID || conv.ChannelID == f.ch1.ID {
			t.Fatalf("server conversation leaked into DM sidebar: %+v", conv)
		}
	}

	var contentBefore string
	if err := f.db.QueryRowContext(f.ctx, `SELECT content FROM conversation_messages WHERE id = $1`, messageID).Scan(&contentBefore); err != nil {
		t.Fatalf("seed content: %v", err)
	}

	// Sequential probing around a known ID: message probes all fail as
	// "server message not found", channel probes as "server channel not found".
	messageProbes := []protocol.Control{
		{Type: "UPDATE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(messageID - 1), ServerContent: "probe"},
		{Type: "UPDATE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(messageID + 1), ServerContent: "probe"},
		{Type: "DELETE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(messageID - 1)},
		{Type: "DELETE_SERVER_MESSAGE", ServerMessageID: protocol.MessageID(messageID + 1)},
		{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(messageID - 1)},
		{Type: "MARK_SERVER_MESSAGE_READ", ChannelID: f.ch1.ID, ServerMessageID: protocol.MessageID(messageID + 1)},
	}
	for _, req := range messageProbes {
		sendTest(t, outsider, req)
		f.expectError(t, outsider, "server message not found")
	}
	channelProbes := []protocol.Control{
		{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID - 1},
		{Type: "LIST_SERVER_MESSAGES", ChannelID: f.ch1.ID + 1},
		{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID - 1, ServerContent: "probe"},
		{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID + 1, ServerContent: "probe"},
	}
	for _, req := range channelProbes {
		sendTest(t, outsider, req)
		f.expectError(t, outsider, "server channel not found")
	}

	var contentAfter string
	if err := f.db.QueryRowContext(f.ctx, `SELECT content FROM conversation_messages WHERE id = $1`, messageID).Scan(&contentAfter); err != nil {
		t.Fatalf("seed content after probes: %v", err)
	}
	if contentAfter != contentBefore {
		t.Fatalf("probe changed content to %q, want %q", contentAfter, contentBefore)
	}
	var outsiderRows int
	if err := f.db.QueryRowContext(f.ctx, `
		SELECT count(*) FROM conversation_members cm
		JOIN conversations c ON c.id = cm.conversation_id
		WHERE c.server_channel_id = $1 AND cm.user_id = $2`, f.ch1.ID, f.ids[2]).Scan(&outsiderRows); err != nil {
		t.Fatalf("outsider membership count: %v", err)
	}
	if outsiderRows != 0 {
		t.Fatalf("outsider probes created %d membership rows", outsiderRows)
	}
	stored := f.messageCount(t, f.ch1.ID)
	if stored != 1 {
		t.Fatalf("probes stored %d messages, want only the seed", stored)
	}

	// No rejected request may emit a push to a legitimate member.
	for _, c := range []net.Conn{owner, member} {
		c.SetReadDeadline(time.Now().Add(150 * time.Millisecond))
		_, err := protocol.ReadFrame(c)
		c.SetReadDeadline(time.Time{})
		if err == nil {
			t.Fatal("rejected probe emitted an unexpected frame")
		}
	}
}

// TestLargeServerChannelDeleteCascade checks that deleting a busy text channel
// does not lock the rest of the server: a write on another channel still gets
// an ACK while the cascade removes all rows of the deleted one.
func TestLargeServerChannelDeleteCascade(t *testing.T) {
	f := setupChatFixture(t)
	const synthetic = 100_000

	sendTest(t, f.conns[0], seal(t, f.conns[0], protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID,
		ChannelID: f.ch1.ID, ServerContent: "busy channel"}))
	awaitControl(t, f.conns[0], "seed ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	awaitControl(t, f.conns[1], "seed push", func(m protocol.Control) bool { return m.Type == "SERVER_MESSAGE" })

	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	var convID int
	if err := f.db.QueryRowContext(ctx, `SELECT id FROM conversations WHERE server_channel_id = $1`, f.ch1.ID).Scan(&convID); err != nil {
		t.Fatalf("conversation lookup: %v", err)
	}
	if _, err := f.db.ExecContext(ctx, `
		INSERT INTO conversation_messages (conversation_id, author_id, content)
		SELECT $1, $2, 'synthetic ' || g FROM generate_series(1, $3) g`, convID, f.ids[0], synthetic); err != nil {
		t.Fatalf("bulk insert: %v", err)
	}

	started := time.Now()
	if _, err := f.db.ExecContext(ctx, `DELETE FROM server_channels WHERE id = $1`, f.ch1.ID); err != nil {
		t.Fatalf("delete channel: %v", err)
	}
	t.Logf("cascade delete of %d messages took %s", synthetic, time.Since(started))

	for _, tc := range []struct {
		name  string
		query string
		args  []any
		want  int
	}{
		{"deleted conversations", `SELECT count(*) FROM conversations WHERE server_channel_id = $1`, []any{f.ch1.ID}, 0},
		{"deleted messages", `SELECT count(*) FROM conversation_messages WHERE conversation_id = $1`, []any{convID}, 0},
		{"deleted memberships", `SELECT count(*) FROM conversation_members WHERE conversation_id = $1`, []any{convID}, 0},
		{"other channels", `SELECT count(*) FROM server_channels WHERE server_id = $1`, []any{f.srv1.ID}, 2},
		{"dm", `
			SELECT count(*) FROM conversations
			WHERE kind = 'direct'
			  AND user_low = LEAST($1::bigint, $2::bigint)
			  AND user_high = GREATEST($1::bigint, $2::bigint)`,
			[]any{f.ids[0], f.ids[1]}, 1},
	} {
		var got int
		if err := f.db.QueryRowContext(ctx, tc.query, tc.args...).Scan(&got); err != nil {
			t.Fatalf("count %s: %v", tc.name, err)
		}
		if got != tc.want {
			t.Fatalf("%s = %d, want %d", tc.name, got, tc.want)
		}
	}

	// A write on a surviving channel must still succeed after the cascade.
	sendTest(t, f.conns[0], seal(t, f.conns[0], protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv2.ID,
		ChannelID: f.ch2.ID, ServerContent: "still alive"}))
	alive := awaitControl(t, f.conns[0], "write after cascade", func(m protocol.Control) bool { return m.Type == "ACK" && m.ServerInfo != nil })
	if alive.ServerInfo.ChannelID != protocol.ChannelID(f.ch2.ID) {
		t.Fatalf("write after cascade landed on channel %d, want %d", alive.ServerInfo.ChannelID, f.ch2.ID)
	}
}
