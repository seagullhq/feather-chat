package room

import (
	"bytes"
	"crypto/cipher"
	"net"
	"testing"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

func serveMedia(t *testing.T) (*Manager, *net.UDPAddr) {
	t.Helper()
	m := NewManager()
	srv, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { srv.Close() })
	go m.ServeUDP(srv)
	return m, srv.LocalAddr().(*net.UDPAddr)
}

// peer is the client side of one session's media channel.
type peer struct {
	t        *testing.T
	sess     *Session
	conn     *net.UDPConn
	up, down cipher.AEAD
	counter  uint64
	seen     protocol.ReplayWindow
}

func newPeer(t *testing.T, m *Manager, srv *net.UDPAddr, ssrc uint32) *peer {
	t.Helper()
	s := NewSession(nil)
	up, down, ok := m.Claim(s, ssrc, "u")
	if !ok {
		t.Fatal("claim")
	}
	return &peer{t: t, sess: s, conn: dialMedia(t, srv), up: mustAEAD(up), down: mustAEAD(down)}
}

func dialMedia(t *testing.T, srv *net.UDPAddr) *net.UDPConn {
	t.Helper()
	c, err := net.DialUDP("udp", nil, srv)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { c.Close() })
	return c
}

func (p *peer) seal(kind uint8, payload []byte) []byte {
	b := protocol.SealMedia(p.up, protocol.Header{Kind: kind, SSRC: p.sess.SSRC}, p.counter, payload)
	p.counter++
	return b
}

func (p *peer) write(b []byte) {
	p.t.Helper()
	if _, err := p.conn.Write(b); err != nil {
		p.t.Fatal(err)
	}
}

// read opens the next datagram, checking it with the downlink key and counter.
func (p *peer) read() (protocol.Header, []byte) {
	p.t.Helper()
	buf := make([]byte, 2048)
	p.conn.SetReadDeadline(time.Now().Add(time.Second))
	n, err := p.conn.Read(buf)
	if err != nil {
		p.t.Fatal(err)
	}
	counter, payload, ok := protocol.OpenMedia(p.down, buf[:n])
	if !ok {
		p.t.Fatal("downlink datagram does not open")
	}
	if _, ok := p.seen.Accept(counter); !ok {
		p.t.Fatalf("downlink counter %d reused", counter)
	}
	h, _ := protocol.DecodeHeader(buf[:n])
	return h, payload
}

// ping sends a PING and checks its echo, first answering the address challenge
// if the server sends one.
func (p *peer) ping() {
	p.t.Helper()
	ts := []byte{0, 0, 0, 0, 0, 0, 0, byte(p.counter)}
	p.write(p.seal(protocol.KindPing, ts))
	h, got := p.read()
	if h.Kind == protocol.KindPath {
		p.write(p.seal(protocol.KindPath, got))
		p.write(p.seal(protocol.KindPing, ts))
		h, got = p.read()
	}
	if h.Kind != protocol.KindPing || h.SSRC != p.sess.SSRC || !bytes.Equal(got, ts) {
		p.t.Fatalf("echo = %+v %x", h, got)
	}
}

func (p *peer) expectSilence() {
	p.t.Helper()
	p.conn.SetReadDeadline(time.Now().Add(150 * time.Millisecond))
	if n, err := p.conn.Read(make([]byte, 2048)); err == nil {
		p.t.Fatalf("unexpected datagram of %d bytes", n)
	}
}

func TestUDPForwardResealed(t *testing.T) {
	m, srv := serveMedia(t)
	a, b, c := newPeer(t, m, srv, 1), newPeer(t, m, srv, 2), newPeer(t, m, srv, 3)
	for _, p := range []*peer{a, b, c} {
		m.Join(p.sess, "r")
	}
	a.ping()
	b.ping()
	// c never sent anything, so the server has no address for it.

	h := protocol.Header{Kind: protocol.KindAudio, SSRC: 1, Terminator: true}
	a.write(protocol.SealMedia(a.up, h, a.counter, []byte("opus")))
	got, payload := b.read()
	h.Version = protocol.MediaVersion
	if got != h || string(payload) != "opus" {
		t.Fatalf("forwarded %+v %q", got, payload)
	}
	a.expectSilence()
	c.expectSilence()
}

// Forged, replayed and reordered datagrams must never move a session's address.
func TestUDPAddressOnlyFromFreshAuthenticated(t *testing.T) {
	m, srv := serveMedia(t)
	a, b := newPeer(t, m, srv, 1), newPeer(t, m, srv, 2)
	m.Join(a.sess, "r")
	m.Join(b.sess, "r")
	a.ping()
	b.ping()
	old := b.seal(protocol.KindAudio, []byte("late")) // delivered after a newer one
	captured := b.seal(protocol.KindPing, make([]byte, 8))
	b.write(captured)
	b.read()
	_, addr := m.target(b.sess)

	attacker := &peer{t: t, conn: dialMedia(t, srv)}
	forgedKey := mustAEAD(make([]byte, protocol.MediaKeyLen))
	header := protocol.Header{Kind: protocol.KindPing, SSRC: 2}
	tampered := bytes.Clone(captured)
	tampered[protocol.SealedHeaderLen-1] += 10 // fresh counter, bad tag
	for _, datagram := range [][]byte{
		protocol.SealMedia(forgedKey, header, 100, make([]byte, 8)),
		protocol.SealMedia(a.up, header, 101, make([]byte, 8)), // another session's key
		tampered,
		captured, // replay
		append(protocol.EncodeHeader(header), make([]byte, 30)...), // v1 plaintext
		make([]byte, 4000), // oversized: must not stop the media loop
	} {
		attacker.write(datagram)
	}
	attacker.expectSilence() // unauthenticated datagrams get no reply
	if _, now := m.target(b.sess); now.String() != addr.String() {
		t.Fatalf("address moved to %v", now)
	}

	// An authentic but reordered datagram is forwarded, yet moves nothing.
	attacker.write(old)
	if _, payload := a.read(); string(payload) != "late" {
		t.Fatalf("reordered datagram: %q", payload)
	}
	if _, now := m.target(b.sess); now.String() != addr.String() {
		t.Fatalf("reordered datagram moved the address to %v", now)
	}
	// Its replay is not forwarded.
	attacker.write(old)
	a.expectSilence()

	// The forged counters did not touch the replay window: b still works, and
	// a fresh datagram from a new socket moves the address.
	b.conn = dialMedia(t, srv)
	b.ping()
	if _, now := m.target(b.sess); now.String() != b.conn.LocalAddr().String() {
		t.Fatalf("address = %v, want %v", now, b.conn.LocalAddr())
	}
}

// challenged reads a PATH datagram and returns its random payload.
func (p *peer) challenged() []byte {
	p.t.Helper()
	h, payload := p.read()
	if h.Kind != protocol.KindPath || h.SSRC != p.sess.SSRC || len(payload) != protocol.PathLen {
		p.t.Fatalf("want a challenge, got %+v %x", h, payload)
	}
	return payload
}

func TestUDPReturnRoutability(t *testing.T) {
	m, srv := serveMedia(t)
	a, b := newPeer(t, m, srv, 1), newPeer(t, m, srv, 2)
	m.Join(a.sess, "r")
	m.Join(b.sess, "r")
	a.ping()
	addrIs := func(c *net.UDPConn) {
		t.Helper()
		_, got := m.target(b.sess)
		if c == nil && got != nil || c != nil && !sameAddr(got, c.LocalAddr().(*net.UDPAddr)) {
			t.Fatalf("validated address = %v", got)
		}
	}

	// First join: a PING earns a challenge, no echo; nothing is adopted or sent.
	b.write(b.seal(protocol.KindPing, make([]byte, 8)))
	first := b.challenged()
	b.expectSilence()
	addrIs(nil)
	a.write(a.seal(protocol.KindAudio, []byte("early")))
	b.expectSilence()
	time.Sleep(pathInterval)
	b.ping() // the first challenge counts as lost: the retry gets another
	old := b.conn
	addrIs(old)

	// New socket (rejoin, NAT rebinding): rate limited, media stays on the old one.
	moved := dialMedia(t, srv)
	b.conn = moved
	time.Sleep(pathInterval)
	b.write(b.seal(protocol.KindAudio, []byte("x")))
	b.write(b.seal(protocol.KindAudio, []byte("y")))
	challenge := b.challenged()
	b.expectSilence()
	if bytes.Equal(challenge, first) {
		t.Fatal("a new challenge must use new random bytes")
	}
	a.read() // x and y are authentic: forwarded to the room
	a.read()
	a.write(a.seal(protocol.KindAudio, []byte("still old")))
	b.expectSilence()
	b.conn = old
	if _, got := b.read(); string(got) != "still old" {
		t.Fatalf("old address got %q", got)
	}

	b.write(b.seal(protocol.KindPath, challenge)) // right bytes, wrong address
	b.conn = moved
	b.write(b.seal(protocol.KindPath, first)) // right address, stale bytes
	b.write(b.seal(protocol.KindPath, challenge[:7]))
	b.expectSilence()
	addrIs(old)

	b.write(b.seal(protocol.KindPing, make([]byte, 8)))
	if retry := b.challenged(); !bytes.Equal(retry, challenge) {
		t.Fatal("a retry to the same address changed the challenge")
	}
	b.write(b.seal(protocol.KindPath, challenge))
	b.ping()
	addrIs(moved)
	a.expectSilence() // PATH is never forwarded
}

func TestClaimOnce(t *testing.T) {
	m := NewManager()
	a, b := NewSession(nil), NewSession(nil)
	if _, _, ok := m.Claim(a, 0, "a"); ok {
		t.Fatal("zero SSRC accepted")
	}
	up, down, ok := m.Claim(a, 1, "a")
	if !ok || len(up) != 32 || len(down) != 32 || bytes.Equal(up, down) {
		t.Fatal("claim")
	}
	for _, ssrc := range []uint32{1, 2} {
		if _, _, ok := m.Claim(a, ssrc, "a"); ok {
			t.Fatal("second claim accepted")
		}
	}
	if _, _, ok := m.Claim(b, 1, "b"); ok {
		t.Fatal("taken SSRC accepted")
	}
	m.Release(a)
	if m.BySSRC(1) != nil {
		t.Fatal("released session still reachable")
	}
	up2, _, ok := m.Claim(b, 1, "b")
	if !ok || bytes.Equal(up, up2) {
		t.Fatal("released SSRC must be claimable with fresh keys")
	}
}
