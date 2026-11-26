package control_test

import (
	"context"
	"fmt"
	"net"
	"strconv"
	"testing"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// dmCallFixture is two accepted friends (plus a stranger) with live control
// connections and no voice room: the starting state of every 1:1 call flow.
type dmCallFixture struct {
	a, b, c net.Conn
	aID     int
	bID     int
	cID     int
}

func newDmCallFixture(t *testing.T) dmCallFixture {
	t.Helper()
	store, db := testStore(t)
	base := fmt.Sprintf("call_%d", time.Now().UnixNano())
	names := [3]string{base + "_a", base + "_b", base + "_c"}
	const password = "integration-password-123"

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		for _, name := range names {
			if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) = LOWER($1)", name); err != nil {
				t.Logf("cleanup user %s: %v", name, err)
			}
		}
	})

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	t.Cleanup(func() { _ = ltcp.Close(); _ = ludp.Close() })
	serveTest(t, ltcp, ludp, store, servers.NewStore(db), friends.NewStore(db), messages.NewStore(db))

	addr := ltcp.Addr().String()
	conns := [3]net.Conn{
		dialControl(t, addr, 0xa110, 0, names[0], names[0]+"@example.test", password, true),
		dialControl(t, addr, 0xb110, 0, names[1], names[1]+"@example.test", password, true),
		dialControl(t, addr, 0xc110, 0, names[2], names[2]+"@example.test", password, true),
	}
	t.Cleanup(func() {
		for _, c := range conns {
			_ = c.Close()
		}
	})

	userID := func(name string) int {
		t.Helper()
		var id int
		if err := db.QueryRow("SELECT id FROM users WHERE LOWER(username) = LOWER($1)", name).Scan(&id); err != nil {
			t.Fatal(err)
		}
		return id
	}

	sendTest(t, conns[0], protocol.Control{Type: "SEND_FRIENDSHIP_REQUEST", Username: names[1]})
	awaitControl(t, conns[0], "SEND_FRIENDSHIP_REQUEST ACK", func(m protocol.Control) bool { return m.Type == "ACK" })
	sendTest(t, conns[1], protocol.Control{Type: "ACCEPT_FRIENDSHIP_REQUEST", UserID: userID(names[0])})
	awaitControl(t, conns[1], "ACCEPT_FRIENDSHIP_REQUEST ACK", func(m protocol.Control) bool { return m.Type == "ACK" })

	return dmCallFixture{
		a: conns[0], b: conns[1], c: conns[2],
		aID: userID(names[0]), bID: userID(names[1]), cID: userID(names[2]),
	}
}

// ring starts a call and checks both sides saw the invitation.
func (f dmCallFixture) ring(t *testing.T) string {
	t.Helper()
	sendTest(t, f.a, protocol.Control{Type: "DM_CALL", DirectRecipientID: strconv.Itoa(f.bID)})
	ack := awaitControl(t, f.a, "DM_CALL ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.Room != ""
	})
	invite := awaitControl(t, f.b, "DM_CALL_INVITE", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_INVITE"
	})
	if invite.UserID != f.aID || invite.Room != ack.Room {
		t.Fatalf("invite = %+v, want caller %d in room %q", invite, f.aID, ack.Room)
	}
	return ack.Room
}

func TestIntegrationDmCallRingAcceptLeave(t *testing.T) {
	f := newDmCallFixture(t)
	room := f.ring(t)

	sendTest(t, f.b, protocol.Control{Type: "DM_ACCEPT_CALL", DirectRecipientID: strconv.Itoa(f.aID)})
	accepted := awaitControl(t, f.a, "DM_CALL_ACCEPTED", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_ACCEPTED"
	})
	if accepted.UserID != f.bID || accepted.Room != room {
		t.Fatalf("accepted = %+v, want callee %d in room %q", accepted, f.bID, room)
	}
	awaitControl(t, f.b, "DM_ACCEPT_CALL ACK", func(m protocol.Control) bool { return m.Type == "ACK" })

	// LEAVE has no ACK; the caller learns the call ended through the push.
	sendTest(t, f.b, protocol.Control{Type: "LEAVE"})
	ended := awaitControl(t, f.a, "DM_CALL_ENDED", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_ENDED"
	})
	if ended.CallReason != "left" || ended.UserID != f.bID {
		t.Fatalf("ended = %+v, want reason left from %d", ended, f.bID)
	}
}

// A 1:1 call joins both sides to a room, so the channel SCREEN_* flow works:
// the peer learns the sharer's SSRC, watches it, and sees the share stop.
func TestIntegrationDmCallScreenShare(t *testing.T) {
	f := newDmCallFixture(t)
	f.ring(t)
	sendTest(t, f.b, protocol.Control{Type: "DM_ACCEPT_CALL", DirectRecipientID: strconv.Itoa(f.aID)})
	awaitControl(t, f.b, "DM_ACCEPT_CALL ACK", func(m protocol.Control) bool { return m.Type == "ACK" })
	awaitControl(t, f.a, "DM_CALL_ACCEPTED", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_ACCEPTED"
	})

	// Nobody shares yet: watching is refused like in a channel.
	sendTest(t, f.b, protocol.Control{Type: "SCREEN_WATCH", SSRC: 0xa110})
	if e := awaitControl(t, f.b, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != "not sharing" {
		t.Fatalf("SCREEN_WATCH error = %q, want not sharing", e.Error)
	}

	sendTest(t, f.a, protocol.Control{Type: "SCREEN_START"})
	awaitControl(t, f.a, "SCREEN_START ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.SSRC == 0xa110
	})
	state := awaitControl(t, f.b, "DM_CALL_STATE sharing", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_STATE" && m.Sharing && m.SSRC == 0xa110
	})
	if state.UserID != f.aID {
		t.Fatalf("share state from %d, want %d", state.UserID, f.aID)
	}

	sendTest(t, f.b, protocol.Control{Type: "SCREEN_WATCH", SSRC: 0xa110})
	awaitControl(t, f.b, "SCREEN_WATCH ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.SSRC == 0xa110
	})
	awaitControl(t, f.a, "SCREEN_KEYREQ", func(m protocol.Control) bool {
		return m.Type == "SCREEN_KEYREQ"
	})

	sendTest(t, f.a, protocol.Control{Type: "SCREEN_STOP"})
	awaitControl(t, f.b, "DM_CALL_STATE share off", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_STATE" && !m.Sharing && m.SSRC == 0xa110
	})
}

func TestIntegrationDmCallReject(t *testing.T) {
	f := newDmCallFixture(t)
	f.ring(t)

	sendTest(t, f.b, protocol.Control{Type: "DM_REJECT_CALL", DirectRecipientID: strconv.Itoa(f.aID)})
	ended := awaitControl(t, f.a, "DM_CALL_ENDED", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_ENDED"
	})
	if ended.CallReason != "rejected" || ended.UserID != f.bID {
		t.Fatalf("ended = %+v, want reason rejected from %d", ended, f.bID)
	}
}

// A client that is already busy locally declines a new ring with REJECT even
// when it happens to be the caller: either participant may end a ringing call.
func TestIntegrationDmCallBusyDecline(t *testing.T) {
	f := newDmCallFixture(t)
	f.ring(t)

	sendTest(t, f.a, protocol.Control{Type: "DM_REJECT_CALL", DirectRecipientID: strconv.Itoa(f.bID)})
	ended := awaitControl(t, f.b, "DM_CALL_ENDED", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_ENDED"
	})
	if ended.CallReason != "rejected" || ended.UserID != f.aID {
		t.Fatalf("ended = %+v, want reason rejected from %d", ended, f.aID)
	}
}

func TestIntegrationDmCallCancel(t *testing.T) {
	f := newDmCallFixture(t)
	f.ring(t)

	sendTest(t, f.a, protocol.Control{Type: "DM_CANCEL_CALL", DirectRecipientID: strconv.Itoa(f.bID)})
	ended := awaitControl(t, f.b, "DM_CALL_ENDED", func(m protocol.Control) bool {
		return m.Type == "DM_CALL_ENDED"
	})
	if ended.CallReason != "cancelled" || ended.UserID != f.aID {
		t.Fatalf("ended = %+v, want reason cancelled from %d", ended, f.aID)
	}
}

func TestIntegrationDmCallRequiresFriendship(t *testing.T) {
	f := newDmCallFixture(t)
	sendTest(t, f.a, protocol.Control{Type: "DM_CALL", DirectRecipientID: strconv.Itoa(f.cID)})
	err := awaitControl(t, f.a, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" })
	if err.Error != "friendship required" {
		t.Fatalf("non-friend call error = %q, want friendship required", err.Error)
	}
}

// A finished call is logged in the pair's chat: both sides get it as a push
// with the other party's id, it carries the outcome and the talk time, it is
// there after a reload, and its author cannot edit it.
func TestIntegrationDmCallLogsOutcomeAndDuration(t *testing.T) {
	f := newDmCallFixture(t)
	f.ring(t)

	sendTest(t, f.b, protocol.Control{Type: "DM_ACCEPT_CALL", DirectRecipientID: strconv.Itoa(f.aID)})
	awaitControl(t, f.b, "DM_ACCEPT_CALL ACK", func(m protocol.Control) bool { return m.Type == "ACK" })
	time.Sleep(1100 * time.Millisecond) // talk time is whole seconds
	sendTest(t, f.b, protocol.Control{Type: "LEAVE"})

	isLog := func(m protocol.Control) bool {
		return m.Type == "DIRECT_MESSAGE" && m.DirectInfo != nil && m.DirectInfo.CallOutcome != ""
	}
	toCaller := awaitControl(t, f.a, "call log to caller", isLog)
	toCallee := awaitControl(t, f.b, "call log to callee", isLog)
	if toCaller.UserID != f.bID || toCallee.UserID != f.aID {
		t.Fatalf("peers = %d / %d, want %d / %d", toCaller.UserID, toCallee.UserID, f.bID, f.aID)
	}
	got := toCaller.DirectInfo
	if got.CallOutcome != "ended" || got.CallSeconds < 1 || got.AuthorID != f.aID {
		t.Fatalf("log = %+v, want ended, >=1s, authored by the caller %d", got, f.aID)
	}
	if toCallee.DirectInfo.ID != got.ID {
		t.Fatalf("callee got message %d, caller %d: not the same row", toCallee.DirectInfo.ID, got.ID)
	}

	// Still there after a reload.
	sendTest(t, f.b, protocol.Control{Type: "RECIEVE_DIRECT_MESSAGE", DirectRecipientID: strconv.Itoa(f.aID)})
	history := awaitControl(t, f.b, "history", func(m protocol.Control) bool {
		return m.Type == "ACK" && len(m.DirectMessages) > 0
	})
	if h := history.DirectMessages[0]; h.ID != got.ID || h.CallOutcome != "ended" || h.CallSeconds != got.CallSeconds {
		t.Fatalf("history[0] = %+v, want the logged call %+v", h, got)
	}

	// An event is not text: the caller cannot rewrite it.
	sendTest(t, f.a, protocol.Control{Type: "UPDATE_DIRECT_MESSAGE", DirectMessageID: got.ID, DirectContent: "edited"})
	awaitControl(t, f.a, "edit refused", func(m protocol.Control) bool { return m.Type == "ERROR" })
}

func TestIntegrationDmCallRejectIsLogged(t *testing.T) {
	f := newDmCallFixture(t)
	f.ring(t)

	sendTest(t, f.b, protocol.Control{Type: "DM_REJECT_CALL", DirectRecipientID: strconv.Itoa(f.aID)})
	logged := awaitControl(t, f.a, "call log", func(m protocol.Control) bool {
		return m.Type == "DIRECT_MESSAGE" && m.DirectInfo != nil && m.DirectInfo.CallOutcome != ""
	})
	if logged.DirectInfo.CallOutcome != "rejected" || logged.DirectInfo.CallSeconds != 0 {
		t.Fatalf("log = %+v, want rejected without duration", logged.DirectInfo)
	}
}
