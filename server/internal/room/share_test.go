package room

import (
	"net"
	"testing"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

func newSess(t *testing.T, m *Manager, ssrc uint32, roomName string) *Session {
	t.Helper()
	s := NewSession(nil)
	if !m.Claim(s, ssrc, "u") {
		t.Fatal("claim")
	}
	m.Join(s, roomName)
	s.UDPAddr = &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: int(ssrc)}
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
	m := NewManager()
	srv, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		t.Fatal(err)
	}
	defer srv.Close()
	go m.ServeUDP(srv)

	mk := func(ssrc uint32) (*Session, *net.UDPConn) {
		c, err := net.DialUDP("udp", nil, srv.LocalAddr().(*net.UDPAddr))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { c.Close() })
		s := NewSession(nil)
		m.Claim(s, ssrc, "u")
		m.Join(s, "r")
		ping := append(protocol.EncodeHeader(protocol.Header{Kind: protocol.KindPing, SSRC: ssrc}), 0, 0, 0, 0, 0, 0, 0, 1)
		c.Write(ping)
		buf := make([]byte, 64)
		c.SetReadDeadline(time.Now().Add(time.Second))
		if _, err := c.Read(buf); err != nil { // echo = UDPAddr learned, pinged
			t.Fatal(err)
		}
		return s, c
	}
	a, ca := mk(1)
	_, cb := mk(2) // watcher
	_, cc := mk(3) // bystander
	m.StartShare(a)
	if m.Watch(m.BySSRC(2), 1) != a {
		t.Fatal("watch")
	}

	pkt := append(protocol.EncodeHeader(protocol.Header{Kind: protocol.KindVideo, SSRC: 1}), make([]byte, protocol.MaxVideoDatagram-protocol.HeaderLen)...)
	ca.Write(pkt)

	buf := make([]byte, 2048)
	cb.SetReadDeadline(time.Now().Add(time.Second))
	if n, err := cb.Read(buf); err != nil || n != len(pkt) {
		t.Fatalf("watcher got n=%d err=%v", n, err)
	}
	cc.SetReadDeadline(time.Now().Add(150 * time.Millisecond))
	if _, err := cc.Read(buf); err == nil {
		t.Fatal("bystander must not receive video")
	}
}
