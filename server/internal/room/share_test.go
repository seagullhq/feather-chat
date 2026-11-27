package room

import (
	"net"
	"testing"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

func newSess(t *testing.T, m *Manager, ssrc uint32, roomName string) *Session {
	t.Helper()
	s := NewSession(nil)
	if _, _, ok := m.Claim(s, ssrc, "u"); !ok {
		t.Fatal("claim")
	}
	m.Join(s, roomName)
	m.setAddr(s, &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: int(ssrc)})
	return s
}

func TestShareWatchLifecycle(t *testing.T) {
	m := NewManager()
	a, b, c := newSess(t, m, 1, "r"), newSess(t, m, 2, "r"), newSess(t, m, 3, "other")

	if m.Watch(b, 1) != nil {
		t.Fatal("watch before share must fail")
	}
	if me, ok := m.StartShare(a); !ok || !me.Sharing {
		t.Fatal("start share")
	}
	if m.Watch(c, 1) != nil {
		t.Fatal("watch across rooms must fail")
	}
	if m.Watch(a, 1) != nil {
		t.Fatal("watch self must fail")
	}
	if m.Watch(b, 1) != a {
		t.Fatal("watch")
	}
	if w := m.Watchers(a); len(w) != 1 || w[0] != b {
		t.Fatalf("watchers=%v", w)
	}
	if len(m.Watchers(b)) != 0 {
		t.Fatal("non-sharer has no watchers")
	}
	m.Unwatch(b, 1)
	if len(m.Watchers(a)) != 0 {
		t.Fatal("unwatch")
	}

	// viewer leaving clears its subscription; sharer leaving clears the share
	m.Watch(b, 1)
	m.Leave(b)
	if len(m.Watchers(a)) != 0 {
		t.Fatal("viewer leave must unsubscribe")
	}
	m.Join(b, "r")
	m.Watch(b, 1)
	m.Leave(a)
	if a.sharing || a.watchers != nil {
		t.Fatal("sharer leave must stop share")
	}
}

func TestShareLimit(t *testing.T) {
	m := NewManager()
	for i := 0; i < MaxSharers; i++ {
		if _, ok := m.StartShare(newSess(t, m, uint32(10+i), "r")); !ok {
			t.Fatalf("sharer %d refused", i)
		}
	}
	if _, ok := m.StartShare(newSess(t, m, 99, "r")); ok {
		t.Fatal("over limit accepted")
	}
}

func TestUDPVideoOnlyToWatchers(t *testing.T) {
	m, srv := serveMedia(t)
	mk := func(ssrc uint32) *peer {
		p := newPeer(t, m, srv, ssrc)
		m.Join(p.sess, "r")
		p.ping() // echo = address learnt, pinged
		return p
	}
	a := mk(1)
	b := mk(2) // watcher
	c := mk(3) // bystander
	m.StartShare(a.sess)
	if m.Watch(m.BySSRC(2), 1) != a.sess {
		t.Fatal("watch")
	}

	payload := make([]byte, protocol.MaxVideoDatagram-protocol.SealedHeaderLen-16)
	pkt := a.seal(protocol.KindVideo, payload)
	if len(pkt) != protocol.MaxVideoDatagram {
		t.Fatalf("sealed length %d", len(pkt))
	}
	a.write(pkt)

	if h, got := b.read(); h.SSRC != 1 || h.Kind != protocol.KindVideo || len(got) != len(payload) {
		t.Fatalf("watcher got %+v len=%d", h, len(got))
	}
	c.expectSilence()
}
