package main

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"testing"
	"time"

	"github.com/golang-migrate/migrate/v4"
	"github.com/golang-migrate/migrate/v4/database/postgres"
	"github.com/golang-migrate/migrate/v4/source/iofs"
	_ "github.com/jackc/pgx/v5/stdlib"
	"github.com/joho/godotenv"
	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/migrations"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// testStore connects to the PostgreSQL database named in .env for real, so the
// control path exercises the actual accounts.Store instead of a stub. It skips
// when no database is reachable, so `go test` still runs on a machine without
// Postgres.
func testStore(t *testing.T) (*accounts.Store, *sql.DB) {
	t.Helper()
	_ = godotenv.Load("../.env")

	db, err := sql.Open("pgx", defaultPostgresURI())
	if err != nil {
		t.Skipf("postgres open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := db.PingContext(ctx); err != nil {
		t.Skipf("postgres unreachable (%s/%s): %v", os.Getenv("POSTGRES_HOST"), os.Getenv("POSTGRES_DB"), err)
	}

	if err := runTestMigrations(db); err != nil {
		t.Fatalf("migrations: %v", err)
	}
	return accounts.NewStore(db), db
}

// runTestMigrations applies the embedded SQL migrations. It deliberately does
// not close the migrate instance: the postgres driver wraps the same *sql.DB
// and closing it would break the stores created right after.
func runTestMigrations(db *sql.DB) error {
	sourceDriver, err := iofs.New(migrations.FS, "sql")
	if err != nil {
		return err
	}
	driver, err := postgres.WithInstance(db, &postgres.Config{})
	if err != nil {
		return err
	}
	m, err := migrate.NewWithInstance("iofs", sourceDriver, "postgres", driver)
	if err != nil {
		return err
	}
	if err := m.Up(); err != nil && !errors.Is(err, migrate.ErrNoChange) {
		return err
	}
	return nil
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
	store, db := testStore(t)

	// Unique per run, so repeated `go test` never trips the unique indexes.
	username := fmt.Sprintf("it_%d", time.Now().UnixNano())
	email := username + "@example.test"
	const password = "integration-password-123"

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) = LOWER($1)", username); err != nil {
			t.Logf("cleanup user %s: %v", username, err)
		}
	})

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	go serve(ltcp, ludp.(*net.UDPConn), store, servers.NewStore(db))

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
	store, db := testStore(t)

	username := fmt.Sprintf("cs_%d", time.Now().UnixNano())
	email := username + "@example.test"
	const password = "integration-password-123"
	name := fmt.Sprintf("srv_%d", time.Now().UnixNano()) // 3-32 chars, per the validator
	authedName := name + "_a"

	// Deleting the owner cascades to its servers and server_members rows.
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) = LOWER($1)", username); err != nil {
			t.Logf("cleanup user %s: %v", username, err)
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := store.Register(ctx, username, email, password); err != nil {
		t.Fatalf("Register: %v", err)
	}
	user, err := store.Authenticate(ctx, username, password)
	if err != nil {
		t.Fatalf("Authenticate: %v", err)
	}

	serverStore := servers.NewStore(db)

	// Store-level regression: a successful insert must return the server, never
	// (nil, nil). This is the assertion the old code would have failed.
	created, err := serverStore.CreateServer(ctx, user.ID, name)
	if err != nil {
		t.Fatalf("CreateServer: %v", err)
	}
	if created == nil {
		t.Fatal("CreateServer returned a nil server with a nil error on success")
	}
	if created.Name != name || created.OwnerID != user.ID {
		t.Fatalf("CreateServer = %+v, want name %q owner %d", created, name, user.ID)
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

	// An authenticated session gets the server identity and invite code back.
	c := dialControl(t, ltcp.Addr().String(), 0x4444, "room", username, email, password, false)
	defer c.Close()
	sendTest(t, c, Control{Type: "CREATE_SERVER", ServerName: authedName})
	ack := awaitControl(t, c, "CREATE_SERVER ACK", func(m Control) bool {
		return m.Type == "ACK" && m.InviteCode != ""
	})
	if ack.ServerID == 0 {
		t.Fatal("CREATE_SERVER ACK has no server ID")
	}
	if ack.Name != authedName {
		t.Fatalf("CREATE_SERVER ACK name = %q, want %q", ack.Name, authedName)
	}
	if len(ack.InviteCode) != 7 {
		t.Fatalf("CREATE_SERVER ACK invite code = %q, want 7 characters", ack.InviteCode)
	}
}
