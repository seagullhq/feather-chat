package control

import (
	"crypto/tls"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/netip"
	"strconv"
	"testing"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
	"github.com/seagullhq/feather-chat/server/internal/tlscert"
)

func TestIPKey(t *testing.T) {
	for addr, want := range map[string]string{
		"192.0.2.7:5000":              "192.0.2.7",
		"[::ffff:192.0.2.7]:5000":     "192.0.2.7",
		"[2001:db8:1:2:3:4:5:6]:5000": "2001:db8:1:2::/64",
		"[2001:db8:1:2:ffff::1]:6000": "2001:db8:1:2::/64",
		"[2001:db8:1:3::1]:6000":      "2001:db8:1:3::/64",
		"[fe80::1%eth0]:6000":         "fe80::/64",
	} {
		a := net.TCPAddrFromAddrPort(netip.MustParseAddrPort(addr))
		if got := ipKey(a); got != want {
			t.Errorf("ipKey(%s) = %q, want %q", addr, got, want)
		}
	}
}

type fakeConn struct{ closed bool }

func (f *fakeConn) Close() error { f.closed = true; return nil }

func mustAcquire(t *testing.T, l *connLimiter, ip string) (*connSlot, *fakeConn) {
	t.Helper()
	c := &fakeConn{}
	slot, ok := l.acquire(ip, c)
	if !ok {
		t.Fatalf("connection from %q refused", ip)
	}
	return slot, c
}

func TestConnLimiter(t *testing.T) {
	var l connLimiter
	var slot *connSlot
	for i := 0; i < maxConnsPerIP; i++ {
		slot, _ = mustAcquire(t, &l, "a")
	}
	if _, ok := l.acquire("a", &fakeConn{}); ok {
		t.Fatal("per-IP limit not enforced")
	}
	l.release(slot)
	l.release(slot) // a second release changes nothing
	if l.total != maxConnsPerIP-1 {
		t.Fatalf("total = %d", l.total)
	}
	mustAcquire(t, &l, "a")
}

func TestUnauthEviction(t *testing.T) {
	var l connLimiter
	_, oldest := mustAcquire(t, &l, "single")
	heavySlot, heavy := mustAcquire(t, &l, "heavy")
	_, heavy2 := mustAcquire(t, &l, "heavy")
	for i := len(l.unauth); i < maxUnauthConns; i++ {
		mustAcquire(t, &l, strconv.Itoa(i))
	}
	// The source holding the most loses its oldest connection first.
	mustAcquire(t, &l, "new1")
	if !heavy.closed || heavy2.closed || oldest.closed {
		t.Fatal("want the oldest connection of the largest source closed")
	}
	// With every source equal, the oldest connection goes.
	mustAcquire(t, &l, "new2")
	if !oldest.closed || heavy2.closed {
		t.Fatal("want the oldest connection closed")
	}
	l.release(heavySlot) // the evicted handler returns
	if l.total != maxUnauthConns || len(l.unauth) != maxUnauthConns {
		t.Fatalf("total = %d, unauth = %d", l.total, len(l.unauth))
	}
}

// A source that has logged in before outlasts strangers, and a crowded network
// loses before a lone address.
func TestEvictionPreference(t *testing.T) {
	var l connLimiter
	slot, _ := mustAcquire(t, &l, "192.0.2.1")
	l.promote(slot)
	l.release(slot)
	_, owner := mustAcquire(t, &l, "192.0.2.1")
	_, lone := mustAcquire(t, &l, "198.51.100.1")
	_, first := mustAcquire(t, &l, "203.0.113.1")
	for i := len(l.unauth); i < maxUnauthConns; i++ {
		mustAcquire(t, &l, "203.0.113."+strconv.Itoa(i))
	}
	mustAcquire(t, &l, "198.51.100.200")
	if !first.closed || lone.closed || owner.closed {
		t.Fatal("want the oldest connection of the crowded network closed")
	}
}

func TestNetKey(t *testing.T) {
	for ip, want := range map[string]string{
		"192.0.2.7":           "192.0.2.0/24",
		"2001:db8:1:2::/64":   "2001:db8:1::/48",
		"not an address here": "not an address here",
	} {
		if got := netKey(ip); got != want {
			t.Errorf("netKey(%s) = %q, want %q", ip, got, want)
		}
	}
}

// Logged-in connections are never evicted and leave room for newcomers.
func TestAuthedNotEvicted(t *testing.T) {
	var l connLimiter
	var authed []*fakeConn
	for i := 0; i < maxConns-1; i++ {
		slot, c := mustAcquire(t, &l, "u"+strconv.Itoa(i))
		l.promote(slot)
		authed = append(authed, c)
	}
	_, waiting := mustAcquire(t, &l, "idle")
	// Total limit reached: the unauthenticated connection makes room.
	slot, _ := mustAcquire(t, &l, "user")
	if !waiting.closed {
		t.Fatal("unauthenticated connection kept at the total limit")
	}
	l.promote(slot)
	if _, ok := l.acquire("late", &fakeConn{}); ok {
		t.Fatal("total limit not enforced")
	}
	for _, c := range authed {
		if c.closed {
			t.Fatal("authenticated connection evicted")
		}
	}
}

func TestKeyedLimiter(t *testing.T) {
	l := newKeyedLimiter(2, 10*time.Second)
	now := time.Now()
	if !l.allow("a", now) || !l.allow("a", now) || l.allow("a", now) {
		t.Fatal("burst of 2 not enforced")
	}
	if !l.allow("b", now) {
		t.Fatal("keys must be independent")
	}
	later := now.Add(10 * time.Second)
	if !l.allow("a", later) || l.allow("a", later) {
		t.Fatal("refill must give exactly one token")
	}
	l.allow("c", now.Add(time.Hour))
	if len(l.buckets) != 1 {
		t.Fatalf("idle buckets not expired: %d left", len(l.buckets))
	}
}

func TestKeyedLimiterReserve(t *testing.T) {
	l := newKeyedLimiter(2, time.Hour)
	// Successes cost nothing.
	for i := 0; i < 5; i++ {
		done, ok := l.reserve("a", time.Now())
		if !ok {
			t.Fatalf("success %d refused", i)
		}
		done(false)
	}
	// Attempts in flight hold their token.
	d1, ok1 := l.reserve("a", time.Now())
	d2, ok2 := l.reserve("a", time.Now())
	if !ok1 || !ok2 {
		t.Fatal("burst refused")
	}
	if _, ok := l.reserve("a", time.Now()); ok {
		t.Fatal("parallel attempts must not exceed the burst")
	}
	d1(true)
	d2(true)
	if _, ok := l.reserve("a", time.Now()); ok {
		t.Fatal("failures must spend the bucket")
	}
}

// startServer serves a control server without stores: only paths that never
// reach the database may be exercised.
func startServer(t *testing.T) (*Server, string) {
	t.Helper()
	cert, err := tlscert.Load("", "", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { l.Close() })
	s := New(nil, nil, nil, nil, room.NewManager())
	go s.Serve(l, tlscert.Config(cert))
	return s, l.Addr().String()
}

func dialTLSTest(t *testing.T, addr string, maxVersion uint16) (*tls.Conn, error) {
	t.Helper()
	c, err := tls.DialWithDialer(&net.Dialer{Timeout: 5 * time.Second}, "tcp", addr,
		&tls.Config{InsecureSkipVerify: true, MaxVersion: maxVersion})
	if err == nil {
		t.Cleanup(func() { c.Close() })
		c.SetDeadline(time.Now().Add(5 * time.Second))
	}
	return c, err
}

func sendFrame(t *testing.T, c net.Conn, msg protocol.Control) {
	t.Helper()
	b, err := json.Marshal(msg)
	if err != nil {
		t.Fatal(err)
	}
	if err := protocol.WriteFrame(c, b); err != nil {
		t.Fatal(err)
	}
}

func recvFrame(t *testing.T, c net.Conn) protocol.Control {
	t.Helper()
	b, err := protocol.ReadFrame(c)
	if err != nil {
		t.Fatal(err)
	}
	var msg protocol.Control
	if err := json.Unmarshal(b, &msg); err != nil {
		t.Fatal(err)
	}
	return msg
}

func TestTLS13Only(t *testing.T) {
	_, addr := startServer(t)
	if _, err := dialTLSTest(t, addr, tls.VersionTLS12); err == nil {
		t.Fatal("TLS 1.2 accepted")
	}
	c, err := dialTLSTest(t, addr, 0)
	if err != nil {
		t.Fatal(err)
	}
	if v := c.ConnectionState().Version; v != tls.VersionTLS13 {
		t.Fatalf("version = %x", v)
	}
}

func TestPreAuthFrameCap(t *testing.T) {
	_, addr := startServer(t)
	c, err := dialTLSTest(t, addr, 0)
	if err != nil {
		t.Fatal(err)
	}
	// A frame of exactly the cap is still read: HELLO is answered.
	payload := make([]byte, preAuthMaxFrame)
	for i := range payload {
		payload[i] = ' '
	}
	copy(payload, `{"type":"HELLO","ssrc":1}`)
	if err := protocol.WriteFrame(c, payload); err != nil {
		t.Fatal(err)
	}
	if m := recvFrame(t, c); m.Type != "ERROR" || m.Error != "authentication required" {
		t.Fatalf("HELLO before LOGIN = %+v", m)
	}
	// One byte more closes the connection on the length prefix alone.
	var hdr [4]byte
	binary.BigEndian.PutUint32(hdr[:], preAuthMaxFrame+1)
	if _, err := c.Write(hdr[:]); err != nil {
		t.Fatal(err)
	}
	var netErr net.Error
	if _, err := protocol.ReadFrame(c); err == nil || (errors.As(err, &netErr) && netErr.Timeout()) {
		t.Fatalf("oversized frame: err = %v, want a closed connection", err)
	}
}

func TestPerIPConnectionLimit(t *testing.T) {
	_, addr := startServer(t)
	for i := 0; i < maxConnsPerIP; i++ {
		if _, err := dialTLSTest(t, addr, 0); err != nil {
			t.Fatalf("connection %d: %v", i, err)
		}
	}
	if _, err := dialTLSTest(t, addr, 0); err == nil {
		t.Fatal("connection over the per-IP limit accepted")
	}
}

// A limited LOGIN or REGISTER must be refused before the store (nil here, so
// reaching it would panic) hashes anything.
func TestAuthRateLimit(t *testing.T) {
	t.Run("per_ip", func(t *testing.T) {
		s, addr := startServer(t)
		for i := 0; i < authBurst; i++ {
			s.authByIP.allow("127.0.0.1", time.Now())
		}
		c, err := dialTLSTest(t, addr, 0)
		if err != nil {
			t.Fatal(err)
		}
		for _, msg := range []protocol.Control{
			{Type: "LOGIN", Identifier: "someone", Password: "password123"},
			{Type: "REGISTER", Username: "someone", Email: "a@example.test", Password: "password123"},
			{Type: "PRELOGIN", Identifier: "someone"},
			{Type: "RECOVER_BEGIN", Identifier: "someone"},
			{Type: "RECOVER_FINISH", Identifier: "someone"},
		} {
			sendFrame(t, c, msg)
			if m := recvFrame(t, c); m.Type != "ERROR" || m.Error != tooManyAttempts {
				t.Fatalf("%s = %+v", msg.Type, m)
			}
		}
	})
}

func TestSecondHelloRefused(t *testing.T) {
	s := New(nil, nil, nil, nil, room.NewManager())
	server, peer := net.Pipe()
	defer server.Close()
	defer peer.Close()
	peer.SetDeadline(time.Now().Add(5 * time.Second))
	c := &client{srv: s, sess: room.NewSession(server)}
	hello := func(ssrc uint32) protocol.Control {
		go s.handleHello(c, protocol.Control{Type: "HELLO", SSRC: ssrc, Name: "x"})
		return recvFrame(t, peer)
	}

	if m := hello(1); m.Type != "ERROR" || m.MediaKeyUp != "" {
		t.Fatalf("HELLO before LOGIN = %+v", m)
	}
	s.rooms.Bind(c.sess, 7, "x")
	first := hello(1)
	if first.Type != "ACK" || first.SSRC != 1 {
		t.Fatalf("first HELLO = %+v", first)
	}
	up, errUp := base64.StdEncoding.DecodeString(first.MediaKeyUp)
	down, errDown := base64.StdEncoding.DecodeString(first.MediaKeyDown)
	if errUp != nil || errDown != nil || len(up) != 32 || len(down) != 32 || first.MediaKeyUp == first.MediaKeyDown {
		t.Fatalf("keys = %q %q", first.MediaKeyUp, first.MediaKeyDown)
	}
	for _, ssrc := range []uint32{1, 2} {
		if m := hello(ssrc); m.Type != "ACK" || m.SSRC != 0 || m.MediaKeyUp != "" || m.MediaKeyDown != "" {
			t.Fatalf("second HELLO(%d) = %+v", ssrc, m)
		}
	}
	if s.rooms.BySSRC(1) != c.sess || s.rooms.BySSRC(2) != nil {
		t.Fatal("refused HELLO changed the claim")
	}
}

// A failed or timed out write leaves a TLS stream unusable: Send closes it.
func TestSendFailureCloses(t *testing.T) {
	server, peer := net.Pipe()
	defer server.Close()
	defer peer.Close()
	if testing.Short() {
		t.Skip("waits for the write timeout")
	}
	// Nobody reads from peer, so the write times out.
	if err := room.NewSession(server).Send(protocol.Control{Type: "ACK"}); err == nil {
		t.Fatal("write without a reader succeeded")
	}
	peer.SetReadDeadline(time.Now().Add(time.Second))
	if _, err := peer.Read(make([]byte, 1)); !errors.Is(err, io.EOF) {
		t.Fatalf("connection not closed: %v", err)
	}
}

// One source spending its failures must not lock the account for the others.
func TestLoginLimitPerSource(t *testing.T) {
	s := New(nil, nil, nil, nil, room.NewManager())
	id := "7"
	for i := 0; i < failedBurst; i++ {
		done, ok := s.reserveLogin(id, "attacker")
		if !ok {
			t.Fatalf("failure %d refused", i)
		}
		done(true)
	}
	if _, ok := s.reserveLogin(id, "attacker"); ok {
		t.Fatal("per-source limit not enforced")
	}
	if _, ok := s.reserveLogin(id, "owner"); !ok {
		t.Fatal("owner locked out")
	}
}
