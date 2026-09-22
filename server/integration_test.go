package main

import (
	"encoding/json"
	"net"
	"testing"
	"time"
)

// dialControl connects a fake client over TCP and sends HELLO then JOIN.
func dialControl(t *testing.T, addr string, ssrc uint32, room string) net.Conn {
	t.Helper()
	c, err := net.Dial("tcp", addr)
	if err != nil {
		t.Fatal(err)
	}
	sendTest(t, c, Control{Type: "HELLO", SSRC: ssrc, Name: "x"})
	sendTest(t, c, Control{Type: "JOIN", Room: room})
	return c
}

func sendTest(t *testing.T, w net.Conn, c Control) {
	t.Helper()
	b, err := json.Marshal(c)
	if err != nil {
		t.Fatal(err)
	}
	writeFrame(w, b)
}

func TestIntegrationTwoClients(t *testing.T) {
	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	go serve(ltcp, ludp.(*net.UDPConn))

	udpAddr := ludp.LocalAddr().String()

	a := dialControl(t, ltcp.Addr().String(), 0x1111, "room")
	b := dialControl(t, ltcp.Addr().String(), 0x2222, "room")
	defer a.Close()
	defer b.Close()
	t.Log("both clients joined room")
	time.Sleep(50 * time.Millisecond) // let JOINs register before media

	// A and B bind their UDP sockets by sending a PING, so the server learns
	// their addr and both clear the audio gate (§5: no AUDIO before the first
	// PING echo). The sockets stay open: the server forwards to these addrs.
	audp, _ := net.Dial("udp", udpAddr)
	defer audp.Close()
	audp.Write(append(EncodeHeader(Header{Kind: KindPing, SSRC: 0x1111}), make([]byte, 8)...))

	budp, _ := net.Dial("udp", udpAddr)
	defer budp.Close()
	budp.Write(append(EncodeHeader(Header{Kind: KindPing, SSRC: 0x2222}), make([]byte, 8)...))

	for _, u := range []net.Conn{audp, budp} {
		u.SetReadDeadline(time.Now().Add(time.Second))
		echo := make([]byte, 1024)
		u.Read(echo) // drain each PING echo that opened that NAT mapping
	}

	// A sends an AUDIO frame; server forwards verbatim to B.
	audp.Write(append(EncodeHeader(Header{Kind: KindAudio, SSRC: 0x1111}), []byte("opusframe")...))

	budp.SetReadDeadline(time.Now().Add(time.Second))
	buf := make([]byte, 1024)
	n, err := budp.Read(buf)
	if err != nil {
		t.Fatal("B did not receive forwarded audio:", err)
	}
	h, _ := DecodeHeader(buf[:n])
	t.Logf("B received %d bytes: %+v", n, h)
	if h.SSRC != 0x1111 || h.Kind != KindAudio {
		t.Fatalf("B got header %+v, want SSRC 0x1111 audio", h)
	}
	if string(buf[n-len("opusframe"):n]) != "opusframe" {
		t.Fatalf("payload not forwarded verbatim: %q", buf[:n])
	}
}
