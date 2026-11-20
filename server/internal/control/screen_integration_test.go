package control_test

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"strconv"
	"testing"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// screenFixture brings two logged-in clients into the same voice room over real
// TCP. Like the other integration tests it needs Postgres and applies the
// migrations through testStore; it is meant to be run only after reviewing them.
func screenFixture(t *testing.T) (net.Conn, net.Conn) {
	t.Helper()
	store, db := testStore(t)

	username := fmt.Sprintf("screen_%d", time.Now().UnixNano())
	email := username + "@example.test"
	const password = "screen-password-123"
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) = LOWER($1)", username); err != nil {
			t.Logf("cleanup user %s: %v", username, err)
		}
	})

	ltcp, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	ludp, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = ltcp.Close()
		_ = ludp.Close()
	})
	serveTest(t, ltcp, ludp, store, servers.NewStore(db))

	a := dialControl(t, ltcp.Addr().String(), 0x1111, 0, username, email, password, true)
	b := dialControl(t, ltcp.Addr().String(), 0x2222, 0, username, email, password, false)
	t.Cleanup(func() {
		_ = a.Close()
		_ = b.Close()
	})

	sendTest(t, a, protocol.Control{Type: "CREATE_SERVER", ServerName: "s" + username})
	srv := awaitControl(t, a, "CREATE_SERVER ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.InviteCode != ""
	})
	var voiceType int
	if err := db.QueryRow("SELECT id FROM channel_types WHERE type_name = 'voice'").Scan(&voiceType); err != nil {
		t.Fatal(err)
	}
	sendTest(t, a, protocol.Control{
		Type: "CREATE_CHANNEL", ServerID: srv.ServerID,
		ChannelName: "screen-room", ChannelType: strconv.Itoa(voiceType),
	})
	awaitControl(t, a, "CREATE_CHANNEL ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.ChannelName != ""
	})
	var channelID int
	if err := db.QueryRow("SELECT id FROM server_channels WHERE server_id = $1", srv.ServerID).Scan(&channelID); err != nil {
		t.Fatal(err)
	}
	for _, c := range []net.Conn{a, b} {
		sendTest(t, c, protocol.Control{Type: "JOIN", ChannelID: channelID})
		awaitRoom(t, c)
	}
	return a, b
}

func TestIntegrationScreenHandlersTCP(t *testing.T) {
	a, b := screenFixture(t)

	// Watching someone who is not sharing must be refused.
	sendTest(t, b, protocol.Control{Type: "SCREEN_WATCH", SSRC: 0x1111})
	if e := awaitControl(t, b, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != "not sharing" {
		t.Fatalf("SCREEN_WATCH error = %q, want %q", e.Error, "not sharing")
	}

	// START acknowledges the presenter and pushes the sharing roster flag.
	sendTest(t, a, protocol.Control{Type: "SCREEN_START"})
	awaitControl(t, a, "SCREEN_START ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.SSRC == 0x1111
	})
	state := awaitControl(t, b, "MEMBER_STATE sharing=true", func(m protocol.Control) bool {
		return m.Type == "MEMBER_STATE" && m.Member != nil &&
			m.Member.SSRC == 0x1111 && m.Member.Sharing
	})
	if state.ChannelID == 0 {
		t.Fatal("MEMBER_STATE is missing channelId")
	}

	// WATCH subscribes B and asks A for a keyframe automatically.
	sendTest(t, b, protocol.Control{Type: "SCREEN_WATCH", SSRC: 0x1111})
	awaitControl(t, b, "SCREEN_WATCH ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.SSRC == 0x1111
	})
	awaitControl(t, a, "automatic SCREEN_KEYREQ", func(m protocol.Control) bool {
		return m.Type == "SCREEN_KEYREQ"
	})

	// An explicit KEYREQ reaches the presenter, but a second one inside the
	// 500 ms gap is dropped (the same limit the unit test checks).
	sendTest(t, b, protocol.Control{Type: "SCREEN_KEYREQ", SSRC: 0x1111})
	awaitControl(t, a, "explicit SCREEN_KEYREQ", func(m protocol.Control) bool {
		return m.Type == "SCREEN_KEYREQ"
	})
	sendTest(t, b, protocol.Control{Type: "SCREEN_KEYREQ", SSRC: 0x1111})
	a.SetReadDeadline(time.Now().Add(200 * time.Millisecond))
	extra, err := protocol.ReadFrame(a)
	a.SetReadDeadline(time.Time{})
	if err == nil {
		var m protocol.Control
		if json.Unmarshal(extra, &m) == nil && m.Type == "SCREEN_KEYREQ" {
			t.Fatal("second SCREEN_KEYREQ was not rate limited")
		}
	}

	sendTest(t, b, protocol.Control{Type: "SCREEN_UNWATCH", SSRC: 0x1111})
	sendTest(t, a, protocol.Control{Type: "SCREEN_STOP"})
	awaitControl(t, b, "MEMBER_STATE sharing=false", func(m protocol.Control) bool {
		return m.Type == "MEMBER_STATE" && m.Member != nil &&
			m.Member.SSRC == 0x1111 && !m.Member.Sharing
	})
}
