package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"testing"
	"time"

	"github.com/joho/godotenv"
	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/migrations"
	"github.com/seagullhq/feather-chat/server/internal/servers"
	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

// testStore connects to the MongoDB named in .env for real, so the control
// path exercises the actual accounts.Store instead of a stub. It skips when no
// database is reachable, so `go test` still runs on a machine without Mongo.
func testStore(t *testing.T) (*accounts.Store, *mongo.Database) {
	t.Helper()
	_ = godotenv.Load("../.env")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	client, err := mongo.Connect(options.Client().ApplyURI(defaultMongoURI()))
	if err != nil {
		t.Skipf("mongo connect: %v", err)
	}
	t.Cleanup(func() { _ = client.Disconnect(context.Background()) })

	if err := client.Ping(ctx, nil); err != nil {
		t.Skipf("mongo unreachable (%s): %v", os.Getenv("MONGO_HOST"), err)
	}

	database := client.Database(os.Getenv("MONGO_DBNAME"))
	if err := migrations.Run(ctx, database); err != nil {
		t.Fatalf("migrations: %v", err)
	}
	return accounts.NewStore(database), database
}

// dialControl connects a real client over TCP: HELLO, then LOGIN (and REGISTER
// for the first one), then JOIN. TCP ordering guarantees the server handles
// them in this order, so the JOIN sees an authenticated session.
func dialControl(t *testing.T, addr string, ssrc uint32, room, username, email, password string, register bool) net.Conn {
	t.Helper()
	c, err := net.Dial("tcp", addr)
	if err != nil {
		t.Fatal(err)
	}
	sendTest(t, c, Control{Type: "HELLO", SSRC: ssrc, Name: "x"})
	if register {
		sendTest(t, c, Control{Type: "REGISTER", Username: username, Email: email, Password: password})
	}
	sendTest(t, c, Control{Type: "LOGIN", Identifier: username, Password: password})
	sendTest(t, c, Control{Type: "JOIN", Room: room})
	awaitRoom(t, c)
	return c
}

// awaitRoom reads ACKs until JOIN is confirmed, so the caller knows the
// session is really in the room before it starts sending UDP. The server
// answers HELLO/REGISTER/LOGIN/JOIN in order; the JOIN ACK is the one carrying
// a room name.
func awaitRoom(t *testing.T, c net.Conn) {
	t.Helper()
	c.SetReadDeadline(time.Now().Add(10 * time.Second))
	defer c.SetReadDeadline(time.Time{})
	for {
		payload, err := readFrame(c)
		if err != nil {
			t.Fatalf("awaiting JOIN ACK: %v", err)
		}
		var m Control
		if json.Unmarshal(payload, &m) != nil {
			continue
		}
		if m.Type == "ERROR" {
			t.Fatalf("server rejected control message: %s", m.Error)
		}
		if m.Type == "ACK" && m.Room != "" {
			return
		}
	}
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
	store, database := testStore(t)

	// Unique per run, so repeated `go test` never trips the unique indexes.
	username := fmt.Sprintf("it_%d", time.Now().UnixNano())
	email := username + "@example.test"
	const password = "integration-password-123"

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_, _ = database.Collection("users").DeleteOne(ctx, bson.D{{Key: "username", Value: username}})
	})

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	go serve(ltcp, ludp.(*net.UDPConn), store, servers.NewStore(database))

	udpAddr := ludp.LocalAddr().String()

	// A registers the real user; B logs into the same account.
	a := dialControl(t, ltcp.Addr().String(), 0x1111, "room", username, email, password, true)
	b := dialControl(t, ltcp.Addr().String(), 0x2222, "room", username, email, password, false)
	defer a.Close()
	defer b.Close()
	t.Log("both clients registered/logged in and joined room")
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

	// B leaves the room; A's next audio must not reach it. Before the fix LEAVE
	// was deferred until the connection closed, so B kept receiving media.
	sendTest(t, b, Control{Type: "LEAVE"})
	time.Sleep(100 * time.Millisecond)
	audp.Write(append(EncodeHeader(Header{Kind: KindAudio, SSRC: 0x1111}), []byte("afterleave")...))
	budp.SetReadDeadline(time.Now().Add(300 * time.Millisecond))
	if n, err := budp.Read(buf); err == nil {
		t.Fatalf("B received %d bytes after leaving the room", n)
	}
}

// awaitControl reads whole control frames until one satisfies want, failing on a
// read timeout or on an unexpected ERROR (unless the caller waits for one).
func awaitControl(t *testing.T, c net.Conn, what string, want func(Control) bool) Control {
	t.Helper()
	c.SetReadDeadline(time.Now().Add(10 * time.Second))
	defer c.SetReadDeadline(time.Time{})
	for {
		payload, err := readFrame(c)
		if err != nil {
			t.Fatalf("awaiting %s: %v", what, err)
		}
		var m Control
		if json.Unmarshal(payload, &m) != nil {
			continue
		}
		if m.Type == "ERROR" && what != "ERROR" {
			t.Fatalf("awaiting %s: server returned ERROR %q", what, m.Error)
		}
		if want(m) {
			return m
		}
	}
}

// TestIntegrationCreateServer covers CREATE_SERVER end to end. Before the fix a
// successful insert returned a nil server, so the ACK path dereferenced nil and
// panicked the whole process; the request was also not gated on authentication.
func TestIntegrationCreateServer(t *testing.T) {
	store, database := testStore(t)

	username := fmt.Sprintf("cs_%d", time.Now().UnixNano())
	email := username + "@example.test"
	const password = "integration-password-123"
	name := fmt.Sprintf("srv_%d", time.Now().UnixNano()) // 3-32 chars, per the validator
	authedName := name + "_a"

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_, _ = database.Collection("users").DeleteOne(ctx, bson.D{{Key: "username", Value: username}})
		_, _ = database.Collection("servers").DeleteMany(ctx, bson.D{{Key: "name", Value: bson.D{{Key: "$in", Value: bson.A{name, authedName}}}}})
	})

	serverStore := servers.NewStore(database)

	// Store-level regression: a successful insert must return the server, never
	// (nil, nil). This is the assertion the old code would have failed.
	created, err := serverStore.CreateServer(context.Background(), bson.NewObjectID(), name)
	if err != nil {
		t.Fatalf("CreateServer: %v", err)
	}
	if created == nil {
		t.Fatal("CreateServer returned a nil server with a nil error on success")
	}
	if len(created.InviteCode) != 7 {
		t.Fatalf("invite code = %q, want 7 characters", created.InviteCode)
	}

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	go serve(ltcp, ludp.(*net.UDPConn), store, serverStore)

	// CREATE_SERVER before LOGIN must be refused.
	anon, err := net.Dial("tcp", ltcp.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer anon.Close()
	sendTest(t, anon, Control{Type: "HELLO", SSRC: 0x3333, Name: "anon"})
	sendTest(t, anon, Control{Type: "CREATE_SERVER", ServerName: "refused"})
	if e := awaitControl(t, anon, "ERROR", func(m Control) bool { return m.Type == "ERROR" }); e.Error != "authentication required" {
		t.Fatalf("anonymous CREATE_SERVER error = %q, want %q", e.Error, "authentication required")
	}

	// An authenticated session gets the invite code back.
	c := dialControl(t, ltcp.Addr().String(), 0x4444, "room", username, email, password, true)
	defer c.Close()
	sendTest(t, c, Control{Type: "CREATE_SERVER", ServerName: authedName})
	ack := awaitControl(t, c, "CREATE_SERVER ACK", func(m Control) bool {
		return m.Type == "ACK" && m.InviteCode != ""
	})
	if len(ack.InviteCode) != 7 {
		t.Fatalf("CREATE_SERVER ACK invite code = %q, want 7 characters", ack.InviteCode)
	}
}
