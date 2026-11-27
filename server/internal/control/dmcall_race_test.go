package control

import (
	"database/sql"
	"encoding/json"
	"net"
	"sync"
	"testing"

	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
)

// dmTestServer has a message store whose database refuses connections, so call
// logging fails fast and these tests need no Postgres.
func dmTestServer(t *testing.T) *Server {
	t.Helper()
	db, err := sql.Open("pgx", "postgres://u:p@127.0.0.1:1/none?connect_timeout=1")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = db.Close() })
	return New(nil, nil, nil, messages.NewStore(db), room.NewManager())
}

// dmTestClient is a logged-in, HELLO'd client. The channel yields what the
// server pushes to it and closes after the returned func closes the link.
func dmTestClient(t *testing.T, s *Server, id int) (*client, <-chan protocol.Control, func()) {
	t.Helper()
	conn, peer := net.Pipe()
	got := make(chan protocol.Control, 1024)
	go func() {
		defer close(got)
		for {
			payload, err := protocol.ReadFrame(peer)
			if err != nil {
				return
			}
			var m protocol.Control
			if json.Unmarshal(payload, &m) == nil {
				got <- m
			}
		}
	}()
	t.Cleanup(func() { _ = conn.Close(); _ = peer.Close() })
	c := &client{srv: s, sess: room.NewSession(conn)}
	s.rooms.Bind(c.sess, id, "user")
	if _, _, ok := s.rooms.Claim(c.sess, uint32(id), "user"); !ok {
		t.Fatal("claim refused")
	}
	return c, got, func() { _ = conn.Close() }
}

// ringTest does what handleDmCall does after its friendship checks.
func ringTest(t *testing.T, s *Server, caller *client, callee int) *dmCall {
	t.Helper()
	mark := s.rooms.Ring(caller.sess)
	call, ok := s.calls.begin(dmRoom(caller.sess.UserID, callee), caller.sess.UserID, callee, caller.sess, mark)
	if !ok {
		t.Fatal("begin refused")
	}
	return call
}

func acceptRefused(t *testing.T, s *Server, callee *client, call *dmCall) {
	t.Helper()
	s.handleDmCallAccept(callee, protocol.Control{Type: "DM_ACCEPT_CALL", DirectRecipientID: "1"})
	if m := s.rooms.Members(call.room, nil); len(m) != 0 {
		t.Fatalf("DM room members = %+v, want none", m)
	}
	if s.calls.call(call.room) != nil {
		t.Fatal("call still registered")
	}
}

func TestDmAcceptAfterCallerJoinedChannel(t *testing.T) {
	s := dmTestServer(t)
	caller, _, _ := dmTestClient(t, s, 1)
	callee, _, _ := dmTestClient(t, s, 2)
	call := ringTest(t, s, caller, 2)

	s.leaveRoom(caller) // as handleJoin does
	s.rooms.Join(caller.sess, "ch:7")

	acceptRefused(t, s, callee, call)
	if got := s.rooms.RoomOf(caller.sess); got != "ch:7" {
		t.Fatalf("caller room = %q, want ch:7", got)
	}
}

func TestDmAcceptDuringCallerDisconnect(t *testing.T) {
	s := dmTestServer(t)
	caller, _, _ := dmTestClient(t, s, 1)
	callee, _, _ := dmTestClient(t, s, 2)
	call := ringTest(t, s, caller, 2)

	s.leaveRoom(caller) // first step of disconnect; Release has not run yet

	acceptRefused(t, s, callee, call)
}

func TestDmSimultaneousLeaveEndsOnce(t *testing.T) {
	s := dmTestServer(t)
	a, gotA, closeA := dmTestClient(t, s, 1)
	_, gotB, closeB := dmTestClient(t, s, 2)

	counts := make(chan int, 2)
	for _, got := range []<-chan protocol.Control{gotA, gotB} {
		go func() {
			n := 0
			for m := range got {
				if m.Type == "DM_CALL_ENDED" {
					n++
				}
			}
			counts <- n
		}()
	}
	const rounds = 100
	for range rounds {
		call := ringTest(t, s, a, 2)
		s.calls.accept(call.room, 2)
		start := make(chan struct{})
		var wg sync.WaitGroup
		for range 8 {
			wg.Add(1)
			go func() {
				defer wg.Done()
				<-start
				// No except session: both sides see every notification.
				s.onDmCallLeave(&client{}, call.room)
			}()
		}
		close(start)
		wg.Wait()
	}
	closeA()
	closeB()
	for range 2 {
		if n := <-counts; n != rounds {
			t.Fatalf("DM_CALL_ENDED count = %d, want %d", n, rounds)
		}
	}
}

func TestDmRingSurvivesOtherSessions(t *testing.T) {
	s := dmTestServer(t)
	caller, _, _ := dmTestClient(t, s, 1)
	callee, _, _ := dmTestClient(t, s, 2)
	call := ringTest(t, s, caller, 2)

	// Neither an idle session of the caller's user going away nor a late leave
	// of an earlier call ends the ring.
	idle := room.NewSession(nil)
	s.rooms.Bind(idle, 1, "user")
	s.dmCancelPendingFor(idle)
	s.onDmCallLeave(callee, call.room)

	if s.calls.call(call.room) != call {
		t.Fatal("ringing call ended")
	}
}
