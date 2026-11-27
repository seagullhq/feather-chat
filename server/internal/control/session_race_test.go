package control

import (
	"io"
	"net"
	"sync"
	"testing"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
)

// The callee's goroutine joins the caller's session to a DM room while the
// caller's own goroutine keeps handling messages. Only -race can fail this.
func TestAcceptJoinsCallerWhileItHandlesMessagesRace(t *testing.T) {
	s := New(nil, nil, nil, nil, room.NewManager())
	conn, peer := net.Pipe()
	defer conn.Close()
	defer peer.Close()
	go func() { _, _ = io.Copy(io.Discard, peer) }()
	c := &client{srv: s, sess: room.NewSession(conn)}
	s.rooms.Bind(c.sess, 1, "caller")
	if _, _, ok := s.rooms.Claim(c.sess, 1, "caller"); !ok {
		t.Fatal("claim refused")
	}

	const rounds = 200
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for range rounds {
			s.rooms.Join(c.sess, dmRoom(1, 2)) // as handleDmCallAccept does
		}
	}()
	for range rounds {
		s.handleState(c, protocol.Control{Type: "STATE"})
		s.handleScreenStart(c, protocol.Control{Type: "SCREEN_START"})
		s.handleScreenStop(c, protocol.Control{Type: "SCREEN_STOP"})
		s.leaveRoom(c)
	}
	wg.Wait()
}

// A disconnect scans the pending calls while another one is being accepted.
// Only -race can fail this.
func TestDisconnectDuringAcceptRace(t *testing.T) {
	s := New(nil, nil, nil, nil, room.NewManager())
	const rounds = 200
	other := room.NewSession(nil)
	s.rooms.Bind(other, 3, "other")
	for range rounds {
		call, ok := s.calls.begin(dmRoom(1, 2), 1, 2, nil, 0)
		if !ok {
			t.Fatal("begin refused")
		}
		var wg sync.WaitGroup
		wg.Add(1)
		go func() {
			defer wg.Done()
			s.calls.accept(call.room, 2)
		}()
		s.dmCancelPendingFor(other) // a user in neither call
		wg.Wait()
		s.calls.drop(call.room, call)
	}
}
