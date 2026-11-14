package control_test

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"strconv"
	"strings"
	"testing"
	"time"

	_ "github.com/jackc/pgx/v5/stdlib"
	"github.com/joho/godotenv"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/config"
	"github.com/seagullhq/feather-chat/server/internal/control"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/migrations"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// testStore connects to the PostgreSQL database named in .env for real, so the
// control path exercises the actual accounts.Store instead of a stub. It skips
// when no database is reachable, so `go test` still runs on a machine without
// Postgres.
func testStore(t *testing.T) (*accounts.Store, *sql.DB) {
	t.Helper()
	_ = godotenv.Load("../../../.env")

	db, err := sql.Open("pgx", config.PostgresURI())
	if err != nil {
		t.Skipf("postgres open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := db.PingContext(ctx); err != nil {
		t.Skipf("postgres unreachable (%s/%s): %v", os.Getenv("POSTGRES_HOST"), os.Getenv("POSTGRES_DB"), err)
	}

	if _, err := migrations.Up(db); err != nil {
		t.Fatalf("migrations: %v", err)
	}
	return accounts.NewStore(db), db
}

// serveTest starts a whole server (control TCP + media UDP) on the given
// listeners, the same way main wires it.
func serveTest(t *testing.T, ltcp net.Listener, ludp net.PacketConn, users *accounts.Store, serverStore *servers.ServerStore, friendStore *friends.ServerStore) {
	t.Helper()
	rooms := room.NewManager()
	go rooms.ServeUDP(ludp.(*net.UDPConn))
	go control.New(users, serverStore, friendStore, rooms).Serve(ltcp)
}

// dialControl connects a real client over TCP: HELLO, then LOGIN (and REGISTER
// for the first one), then JOIN. TCP ordering guarantees the server handles
// them in this order, so the JOIN sees an authenticated session.
func dialControl(t *testing.T, addr string, ssrc uint32, channelID int, username, email, password string, register bool) net.Conn {
	t.Helper()
	c, err := net.Dial("tcp", addr)
	if err != nil {
		t.Fatal(err)
	}
	sendTest(t, c, protocol.Control{Type: "HELLO", SSRC: ssrc, Name: "x"})
	if register {
		sendTest(t, c, protocol.Control{Type: "REGISTER", Username: username, Email: email, Password: password})
	}
	sendTest(t, c, protocol.Control{Type: "LOGIN", Identifier: username, Password: password})
	// LOGIN ACK carries the username; waiting keeps REGISTER/LOGIN ordered across clients.
	awaitControl(t, c, "LOGIN ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.Name != "" })
	if channelID != 0 {
		sendTest(t, c, protocol.Control{Type: "JOIN", ChannelID: channelID})
		awaitRoom(t, c)
	}
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
		payload, err := protocol.ReadFrame(c)
		if err != nil {
			t.Fatalf("awaiting JOIN ACK: %v", err)
		}
		var m protocol.Control
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

func sendTest(t *testing.T, w net.Conn, c protocol.Control) {
	t.Helper()
	b, err := json.Marshal(c)
	if err != nil {
		t.Fatal(err)
	}
	protocol.WriteFrame(w, b)
}

// awaitControl reads whole control frames until one satisfies want, failing on a
// read timeout or on an unexpected ERROR (unless the caller waits for one).
func awaitControl(t *testing.T, c net.Conn, what string, want func(protocol.Control) bool) protocol.Control {
	t.Helper()
	c.SetReadDeadline(time.Now().Add(10 * time.Second))
	defer c.SetReadDeadline(time.Time{})
	for {
		payload, err := protocol.ReadFrame(c)
		if err != nil {
			t.Fatalf("awaiting %s: %v", what, err)
		}
		var m protocol.Control
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
	serveTest(t, ltcp, ludp, store, servers.NewStore(db), friends.NewStore(db))

	udpAddr := ludp.LocalAddr().String()

	// A registers the real user; B logs into the same account.
	a := dialControl(t, ltcp.Addr().String(), 0x1111, 0, username, email, password, true)
	b := dialControl(t, ltcp.Addr().String(), 0x2222, 0, username, email, password, false)
	defer a.Close()
	defer b.Close()
	sendTest(t, a, protocol.Control{Type: "CREATE_SERVER", ServerName: "v" + username})
	srv := awaitControl(t, a, "CREATE_SERVER ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.InviteCode != "" })
	var voiceType int
	if err := db.QueryRow("SELECT id FROM channel_types WHERE type_name = 'voice'").Scan(&voiceType); err != nil {
		t.Fatal(err)
	}
	sendTest(t, a, protocol.Control{Type: "CREATE_CHANNEL", ServerID: srv.ServerID, ChannelName: "voice-room", ChannelType: strconv.Itoa(voiceType)})
	ch := awaitControl(t, a, "CREATE_CHANNEL ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ChannelName != "" })
	_ = ch
	var channelID int
	if err := db.QueryRow("SELECT id FROM server_channels WHERE server_id = $1", srv.ServerID).Scan(&channelID); err != nil {
		t.Fatal(err)
	}
	for _, c := range []net.Conn{a, b} {
		sendTest(t, c, protocol.Control{Type: "JOIN", ChannelID: channelID})
		awaitRoom(t, c)
	}
	// A channel that is not theirs (or missing) must be refused.
	sendTest(t, a, protocol.Control{Type: "JOIN", ChannelID: channelID + 1000000})
	if e := awaitControl(t, a, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != "channel not found" {
		t.Fatalf("bad JOIN error = %q", e.Error)
	}
	t.Log("both clients registered/logged in and joined room")
	time.Sleep(50 * time.Millisecond) // let JOINs register before media

	// A and B bind their UDP sockets by sending a PING, so the server learns
	// their addr and both clear the audio gate (§5: no AUDIO before the first
	// PING echo). The sockets stay open: the server forwards to these addrs.
	audp, _ := net.Dial("udp", udpAddr)
	defer audp.Close()
	audp.Write(append(protocol.EncodeHeader(protocol.Header{Kind: protocol.KindPing, SSRC: 0x1111}), make([]byte, 8)...))

	budp, _ := net.Dial("udp", udpAddr)
	defer budp.Close()
	budp.Write(append(protocol.EncodeHeader(protocol.Header{Kind: protocol.KindPing, SSRC: 0x2222}), make([]byte, 8)...))

	for _, u := range []net.Conn{audp, budp} {
		u.SetReadDeadline(time.Now().Add(time.Second))
		echo := make([]byte, 1024)
		u.Read(echo) // drain each PING echo that opened that NAT mapping
	}

	// A sends an AUDIO frame; server forwards verbatim to B.
	audp.Write(append(protocol.EncodeHeader(protocol.Header{Kind: protocol.KindAudio, SSRC: 0x1111}), []byte("opusframe")...))

	budp.SetReadDeadline(time.Now().Add(time.Second))
	buf := make([]byte, 1024)
	n, err := budp.Read(buf)
	if err != nil {
		t.Fatal("B did not receive forwarded audio:", err)
	}
	h, _ := protocol.DecodeHeader(buf[:n])
	t.Logf("B received %d bytes: %+v", n, h)
	if h.SSRC != 0x1111 || h.Kind != protocol.KindAudio {
		t.Fatalf("B got header %+v, want SSRC 0x1111 audio", h)
	}
	if string(buf[n-len("opusframe"):n]) != "opusframe" {
		t.Fatalf("payload not forwarded verbatim: %q", buf[:n])
	}

	// B leaves the room; A's next audio must not reach it. Before the fix LEAVE
	// was deferred until the connection closed, so B kept receiving media.
	sendTest(t, b, protocol.Control{Type: "LEAVE"})
	time.Sleep(100 * time.Millisecond)
	audp.Write(append(protocol.EncodeHeader(protocol.Header{Kind: protocol.KindAudio, SSRC: 0x1111}), []byte("afterleave")...))
	budp.SetReadDeadline(time.Now().Add(300 * time.Millisecond))
	if n, err := budp.Read(buf); err == nil {
		t.Fatalf("B received %d bytes after leaving the room", n)
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
	serveTest(t, ltcp, ludp, store, serverStore, friends.NewStore(db))

	// CREATE_SERVER before LOGIN must be refused.
	anon, err := net.Dial("tcp", ltcp.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer anon.Close()
	sendTest(t, anon, protocol.Control{Type: "HELLO", SSRC: 0x3333, Name: "anon"})
	sendTest(t, anon, protocol.Control{Type: "CREATE_SERVER", ServerName: "refused"})
	if e := awaitControl(t, anon, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != "authentication required" {
		t.Fatalf("anonymous CREATE_SERVER error = %q, want %q", e.Error, "authentication required")
	}

	// An authenticated session gets the server identity and invite code back.
	c := dialControl(t, ltcp.Addr().String(), 0x4444, 0, username, email, password, false)
	defer c.Close()
	sendTest(t, c, protocol.Control{Type: "CREATE_SERVER", ServerName: authedName})
	ack := awaitControl(t, c, "CREATE_SERVER ACK", func(m protocol.Control) bool {
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

// TestIntegrationListServers covers the login path: after LOGIN the client asks
// LIST_SERVERS and must get back the servers the user belongs to, and nothing
// from another account.
func TestIntegrationListServers(t *testing.T) {
	store, db := testStore(t)

	owner := fmt.Sprintf("ls_%d", time.Now().UnixNano())
	other := owner + "_other"
	const password = "integration-password-123"
	firstName := fmt.Sprintf("one_%d", time.Now().UnixNano())
	secondName := fmt.Sprintf("two_%d", time.Now().UnixNano())
	otherName := fmt.Sprintf("oth_%d", time.Now().UnixNano())

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) IN (LOWER($1), LOWER($2))", owner, other); err != nil {
			t.Logf("cleanup users %s/%s: %v", owner, other, err)
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := store.Register(ctx, owner, owner+"@example.test", password); err != nil {
		t.Fatalf("Register owner: %v", err)
	}
	if err := store.Register(ctx, other, other+"@example.test", password); err != nil {
		t.Fatalf("Register other: %v", err)
	}
	ownerUser, err := store.Authenticate(ctx, owner, password)
	if err != nil {
		t.Fatalf("Authenticate owner: %v", err)
	}
	otherUser, err := store.Authenticate(ctx, other, password)
	if err != nil {
		t.Fatalf("Authenticate other: %v", err)
	}

	serverStore := servers.NewStore(db)
	if _, err := serverStore.CreateServer(ctx, ownerUser.ID, firstName); err != nil {
		t.Fatalf("CreateServer(first): %v", err)
	}
	if _, err := serverStore.CreateServer(ctx, ownerUser.ID, secondName); err != nil {
		t.Fatalf("CreateServer(second): %v", err)
	}
	if _, err := serverStore.CreateServer(ctx, otherUser.ID, otherName); err != nil {
		t.Fatalf("CreateServer(other): %v", err)
	}

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	serveTest(t, ltcp, ludp, store, serverStore, friends.NewStore(db))

	// LIST_SERVERS before LOGIN must be refused.
	anon, err := net.Dial("tcp", ltcp.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer anon.Close()
	sendTest(t, anon, protocol.Control{Type: "LIST_SERVERS"})
	if e := awaitControl(t, anon, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != "authentication required" {
		t.Fatalf("anonymous LIST_SERVERS error = %q, want %q", e.Error, "authentication required")
	}

	c := dialControl(t, ltcp.Addr().String(), 0x5555, 0, owner, owner+"@example.test", password, false)
	defer c.Close()
	sendTest(t, c, protocol.Control{Type: "LIST_SERVERS"})
	ack := awaitControl(t, c, "LIST_SERVERS ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.Servers != nil
	})

	got := map[string]bool{}
	for _, server := range ack.Servers {
		if server.ID == 0 {
			t.Fatal("LIST_SERVERS returned a zero server ID")
		}
		if len(server.InviteCode) != 7 {
			t.Fatalf("LIST_SERVERS invite code = %q, want 7 characters", server.InviteCode)
		}
		got[server.Name] = true
	}
	if len(got) != 2 || !got[firstName] || !got[secondName] {
		t.Fatalf("LIST_SERVERS returned %v, want %q and %q", got, firstName, secondName)
	}
	if got[otherName] {
		t.Fatalf("LIST_SERVERS leaked another user's server %q", otherName)
	}
}

// TestIntegrationKickAndBan covers member removal: the removed client is told
// directly, the server disappears from its listing, and a banned user cannot
// join again with the invite code.
func TestIntegrationKickAndBan(t *testing.T) {
	store, db := testStore(t)
	owner := fmt.Sprintf("kb_%d", time.Now().UnixNano())
	member := owner + "_m"
	const password = "integration-password-123"
	serverName := fmt.Sprintf("kb_%d", time.Now().UnixNano())

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) IN (LOWER($1), LOWER($2))", owner, member); err != nil {
			t.Logf("cleanup users %s/%s: %v", owner, member, err)
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := store.Register(ctx, owner, owner+"@example.test", password); err != nil {
		t.Fatalf("Register owner: %v", err)
	}
	if err := store.Register(ctx, member, member+"@example.test", password); err != nil {
		t.Fatalf("Register member: %v", err)
	}
	ownerUser, err := store.Authenticate(ctx, owner, password)
	if err != nil {
		t.Fatalf("Authenticate owner: %v", err)
	}
	memberUser, err := store.Authenticate(ctx, member, password)
	if err != nil {
		t.Fatalf("Authenticate member: %v", err)
	}

	serverStore := servers.NewStore(db)
	srv, err := serverStore.CreateServer(ctx, ownerUser.ID, serverName)
	if err != nil {
		t.Fatalf("CreateServer: %v", err)
	}
	if _, err := serverStore.JoinServer(ctx, memberUser.ID, srv.InviteCode); err != nil {
		t.Fatalf("JoinServer: %v", err)
	}

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	serveTest(t, ltcp, ludp, store, serverStore, friends.NewStore(db))

	ownerConn := dialControl(t, ltcp.Addr().String(), 0x6666, 0, owner, owner+"@example.test", password, false)
	defer ownerConn.Close()
	memberConn := dialControl(t, ltcp.Addr().String(), 0x7777, 0, member, member+"@example.test", password, false)
	defer memberConn.Close()

	// Kick: the member gets USER_KICKED and the server leaves their listing.
	sendTest(t, ownerConn, protocol.Control{Type: "KICK_USER_FROM_SERVER", ServerID: srv.ID, UserID: memberUser.ID})
	awaitControl(t, ownerConn, "KICK ACK", func(m protocol.Control) bool { return m.Type == "ACK" })
	kicked := awaitControl(t, memberConn, "USER_KICKED", func(m protocol.Control) bool { return m.Type == "USER_KICKED" })
	if kicked.ServerID != srv.ID {
		t.Fatalf("USER_KICKED server = %d, want %d", kicked.ServerID, srv.ID)
	}
	sendTest(t, memberConn, protocol.Control{Type: "LIST_SERVERS"})
	list := awaitControl(t, memberConn, "LIST_SERVERS ACK", func(m protocol.Control) bool {
		return m.Type == "ACK"
	})
	for _, s := range list.Servers {
		if s.ID == srv.ID {
			t.Fatalf("kicked server %d still listed", srv.ID)
		}
	}

	// Ban: the member gets USER_BANNED, disappears from the roster, and can no
	// longer rejoin with the same invite code.
	if _, err := serverStore.JoinServer(ctx, memberUser.ID, srv.InviteCode); err != nil {
		t.Fatalf("rejoin after kick: %v", err)
	}
	sendTest(t, ownerConn, protocol.Control{Type: "BAN_USER_FROM_SERVER", ServerID: srv.ID, UserID: memberUser.ID})
	awaitControl(t, ownerConn, "BAN ACK", func(m protocol.Control) bool { return m.Type == "ACK" })
	banned := awaitControl(t, memberConn, "USER_BANNED", func(m protocol.Control) bool { return m.Type == "USER_BANNED" })
	if banned.ServerID != srv.ID {
		t.Fatalf("USER_BANNED server = %d, want %d", banned.ServerID, srv.ID)
	}

	sendTest(t, ownerConn, protocol.Control{Type: "LIST_MEMBERS", ServerID: srv.ID})
	roster := awaitControl(t, ownerConn, "LIST_MEMBERS ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.ServerMembers != nil
	})
	for _, m := range roster.ServerMembers {
		if m.ID == memberUser.ID {
			t.Fatalf("banned user %d still in roster", memberUser.ID)
		}
	}

	sendTest(t, memberConn, protocol.Control{Type: "JOIN_SERVER", InviteCode: srv.InviteCode})
	if e := awaitControl(t, memberConn, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != servers.ErrBanned.Error() {
		t.Fatalf("banned rejoin error = %q, want %q", e.Error, servers.ErrBanned.Error())
	}
}

// TestIntegrationMakeAdmin: only the owner may promote a member, the roster
// reports the new role, and the promotion is pushed to the other sessions.
func TestIntegrationMakeAdmin(t *testing.T) {
	store, db := testStore(t)
	owner := fmt.Sprintf("ma_%d", time.Now().UnixNano())
	member := owner + "_m"
	const password = "integration-password-123"
	serverName := fmt.Sprintf("ma_%d", time.Now().UnixNano())

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) IN (LOWER($1), LOWER($2))", owner, member); err != nil {
			t.Logf("cleanup users %s/%s: %v", owner, member, err)
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := store.Register(ctx, owner, owner+"@example.test", password); err != nil {
		t.Fatalf("Register owner: %v", err)
	}
	if err := store.Register(ctx, member, member+"@example.test", password); err != nil {
		t.Fatalf("Register member: %v", err)
	}
	ownerUser, err := store.Authenticate(ctx, owner, password)
	if err != nil {
		t.Fatalf("Authenticate owner: %v", err)
	}
	memberUser, err := store.Authenticate(ctx, member, password)
	if err != nil {
		t.Fatalf("Authenticate member: %v", err)
	}

	serverStore := servers.NewStore(db)
	srv, err := serverStore.CreateServer(ctx, ownerUser.ID, serverName)
	if err != nil {
		t.Fatalf("CreateServer: %v", err)
	}
	if _, err := serverStore.JoinServer(ctx, memberUser.ID, srv.InviteCode); err != nil {
		t.Fatalf("JoinServer: %v", err)
	}

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	serveTest(t, ltcp, ludp, store, serverStore, friends.NewStore(db))

	ownerConn := dialControl(t, ltcp.Addr().String(), 0x8888, 0, owner, owner+"@example.test", password, false)
	defer ownerConn.Close()
	memberConn := dialControl(t, ltcp.Addr().String(), 0x9999, 0, member, member+"@example.test", password, false)
	defer memberConn.Close()

	// A member cannot promote themselves.
	sendTest(t, memberConn, protocol.Control{Type: "MAKE_SERVER_ADMIN", ServerID: srv.ID, UserID: memberUser.ID})
	if e := awaitControl(t, memberConn, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error == "" {
		t.Fatal("member promotion was accepted")
	}

	// The owner promotes the member, who is told the roster is stale.
	sendTest(t, ownerConn, protocol.Control{Type: "MAKE_SERVER_ADMIN", ServerID: srv.ID, UserID: memberUser.ID})
	awaitControl(t, ownerConn, "MAKE_SERVER_ADMIN ACK", func(m protocol.Control) bool { return m.Type == "ACK" })
	changed := awaitControl(t, memberConn, "SERVER_MEMBERS_CHANGED", func(m protocol.Control) bool {
		return m.Type == "SERVER_MEMBERS_CHANGED"
	})
	if changed.ServerID != srv.ID {
		t.Fatalf("SERVER_MEMBERS_CHANGED server = %d, want %d", changed.ServerID, srv.ID)
	}

	sendTest(t, ownerConn, protocol.Control{Type: "LIST_MEMBERS", ServerID: srv.ID})
	roster := awaitControl(t, ownerConn, "LIST_MEMBERS ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.ServerMembers != nil
	})
	var found bool
	for _, m := range roster.ServerMembers {
		if m.ID == memberUser.ID {
			found = true
			if !m.Admin {
				t.Fatalf("member %d is not reported as admin: %+v", memberUser.ID, m)
			}
		}
	}
	if !found {
		t.Fatalf("promoted user %d missing from roster", memberUser.ID)
	}

	// The owner demotes the member again; the roster drops the admin flag.
	sendTest(t, ownerConn, protocol.Control{Type: "MAKE_SERVER_MEMBER", ServerID: srv.ID, UserID: memberUser.ID})
	awaitControl(t, ownerConn, "MAKE_SERVER_MEMBER ACK", func(m protocol.Control) bool { return m.Type == "ACK" })
	changed = awaitControl(t, memberConn, "SERVER_MEMBERS_CHANGED", func(m protocol.Control) bool {
		return m.Type == "SERVER_MEMBERS_CHANGED"
	})
	if changed.ServerID != srv.ID {
		t.Fatalf("SERVER_MEMBERS_CHANGED server = %d, want %d", changed.ServerID, srv.ID)
	}

	sendTest(t, ownerConn, protocol.Control{Type: "LIST_MEMBERS", ServerID: srv.ID})
	roster = awaitControl(t, ownerConn, "LIST_MEMBERS ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.ServerMembers != nil
	})
	for _, m := range roster.ServerMembers {
		if m.ID == memberUser.ID && m.Admin {
			t.Fatalf("demoted member %d still reported as admin: %+v", memberUser.ID, m)
		}
	}
}

// TestIntegrationRoster: JOIN returns who is already there, the others are told
// about joins, state changes and leaves, and an SSRC can't be claimed twice.
func TestIntegrationRoster(t *testing.T) {
	store, db := testStore(t)
	username := fmt.Sprintf("ro_%d", time.Now().UnixNano())
	const password = "integration-password-123"
	t.Cleanup(func() {
		db.Exec("DELETE FROM users WHERE LOWER(username) = LOWER($1)", username)
	})

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	serveTest(t, ltcp, ludp, store, servers.NewStore(db), friends.NewStore(db))
	addr := ltcp.Addr().String()

	a := dialControl(t, addr, 0x1111, 0, username, username+"@example.test", password, true)
	b := dialControl(t, addr, 0x2222, 0, username, "", password, false)
	defer a.Close()
	defer b.Close()

	// A second client asking for A's SSRC must be refused: the ACK has none.
	dup := dialControl(t, addr, 0x1111, 0, username, "", password, false)
	defer dup.Close()
	sendTest(t, dup, protocol.Control{Type: "HELLO", SSRC: 0x1111, Name: "dup"})
	if ack := awaitControl(t, dup, "dup HELLO ACK", func(m protocol.Control) bool { return m.Type == "ACK" }); ack.SSRC != 0 {
		t.Fatalf("duplicate SSRC was granted: %+v", ack)
	}

	sendTest(t, a, protocol.Control{Type: "CREATE_SERVER", ServerName: "r" + username})
	srv := awaitControl(t, a, "CREATE_SERVER ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.InviteCode != "" })
	var voice int
	if err := db.QueryRow("SELECT id FROM channel_types WHERE type_name = 'voice'").Scan(&voice); err != nil {
		t.Fatal(err)
	}
	sendTest(t, a, protocol.Control{Type: "CREATE_CHANNEL", ServerID: srv.ServerID, ChannelName: "roster-room", ChannelType: strconv.Itoa(voice)})
	ch := awaitControl(t, a, "CREATE_CHANNEL ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.ChannelName != "" })
	if ch.ChannelID == 0 {
		t.Fatal("CREATE_CHANNEL ACK has no channelId")
	}

	sendTest(t, a, protocol.Control{Type: "JOIN", ChannelID: ch.ChannelID})
	ackA := awaitControl(t, a, "A JOIN ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.Room != "" })
	if len(ackA.Members) != 0 {
		t.Fatalf("A joined an empty room but sees %v", ackA.Members)
	}
	sendTest(t, b, protocol.Control{Type: "JOIN", ChannelID: ch.ChannelID})
	ackB := awaitControl(t, b, "B JOIN ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.Room != "" })
	if len(ackB.Members) != 1 || ackB.Members[0].SSRC != 0x1111 || ackB.Members[0].Name != username {
		t.Fatalf("B should see A in the roster, got %+v", ackB.Members)
	}
	if j := awaitControl(t, a, "MEMBER_JOINED", func(m protocol.Control) bool { return m.Type == "MEMBER_JOINED" }); j.Member == nil || j.Member.SSRC != 0x2222 || j.ChannelID != ch.ChannelID {
		t.Fatalf("A got %+v", j)
	}

	// LIST_CHANNELS shows the live roster too.
	sendTest(t, dup, protocol.Control{Type: "LIST_CHANNELS", ServerID: srv.ServerID})
	list := awaitControl(t, dup, "LIST_CHANNELS", func(m protocol.Control) bool { return m.Type == "ACK" && m.Channels != nil })
	if len(list.Channels) != 1 || len(list.Channels[0].Members) != 2 {
		t.Fatalf("LIST_CHANNELS roster = %+v", list.Channels)
	}

	yes := true
	sendTest(t, b, protocol.Control{Type: "STATE", Muted: &yes})
	if s := awaitControl(t, a, "MEMBER_STATE", func(m protocol.Control) bool { return m.Type == "MEMBER_STATE" }); s.Member == nil || !s.Member.Muted || s.Member.SSRC != 0x2222 {
		t.Fatalf("A got %+v", s)
	}

	b.Close() // a dropped TCP link must clear the roster too
	if l := awaitControl(t, a, "MEMBER_LEFT", func(m protocol.Control) bool { return m.Type == "MEMBER_LEFT" }); l.SSRC != 0x2222 || l.ChannelID != ch.ChannelID {
		t.Fatalf("A got %+v", l)
	}
}

// TestIntegrationFriendRequest: SEND_FRIENDSHIP_REQUEST stores a pending request
// and refuses a duplicate.
func TestIntegrationFriendRequest(t *testing.T) {
	store, db := testStore(t)
	const password = "integration-password-123"
	base := fmt.Sprintf("fr_%d", time.Now().UnixNano())
	requester, addressee := base+"_a", base+"_b"

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		for _, n := range []string{requester, addressee} {
			if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) = LOWER($1)", n); err != nil {
				t.Logf("cleanup user %s: %v", n, err)
			}
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	ids := make(map[string]int, 2)
	for _, n := range []string{requester, addressee} {
		if err := store.Register(ctx, n, n+"@example.test", password); err != nil {
			t.Fatalf("Register %s: %v", n, err)
		}
		u, err := store.Authenticate(ctx, n, password)
		if err != nil {
			t.Fatalf("Authenticate %s: %v", n, err)
		}
		ids[n] = u.ID
	}

	ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
	ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
	defer ltcp.Close()
	defer ludp.Close()
	serveTest(t, ltcp, ludp, store, servers.NewStore(db), friends.NewStore(db))

	c := dialControl(t, ltcp.Addr().String(), 0xAAAA, 0, requester, requester+"@example.test", password, false)
	defer c.Close()

	sendTest(t, c, protocol.Control{Type: "SEND_FRIENDSHIP_REQUEST", Username: addressee})
	ack := awaitControl(t, c, "SEND_FRIENDSHIP_REQUEST ACK", func(m protocol.Control) bool {
		return m.Type == "ACK" && m.TargetUserID == ids[addressee]
	})
	if ack.TargetUserID != ids[addressee] {
		t.Fatalf("ACK target = %d, want %d", ack.TargetUserID, ids[addressee])
	}

	var status string
	if err := db.QueryRowContext(ctx,
		`SELECT status FROM friendships WHERE requester_id = $1 AND addressee_id = $2`,
		ids[requester], ids[addressee],
	).Scan(&status); err != nil {
		t.Fatalf("stored friend request: %v", err)
	}
	if status != "PENDING" {
		t.Fatalf("stored status = %q, want PENDING", status)
	}

	sendTest(t, c, protocol.Control{Type: "SEND_FRIENDSHIP_REQUEST", Username: addressee})
	if e := awaitControl(t, c, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != friends.ErrAlreadyExists.Error() {
		t.Fatalf("duplicate request error = %q, want %q", e.Error, friends.ErrAlreadyExists.Error())
	}

	sendTest(t, c, protocol.Control{Type: "FETCH_FRIENDSHIP_REQUEST"})
	f := awaitControl(t, c, "FETCH ACK", func(m protocol.Control) bool { return m.Type == "ACK" && len(m.Friends) > 0 })
	if len(f.Friends) != 1 || f.Friends[0].ID != ids[addressee] || f.Friends[0].Relation != "outgoing" {
		t.Fatalf("fetch = %+v, want one outgoing request to %d", f.Friends, ids[addressee])
	}

	sendTest(t, c, protocol.Control{Type: "SEARCH_USERS", Query: strings.ToUpper(base) + "_B"})
	u := awaitControl(t, c, "SEARCH ACK", func(m protocol.Control) bool { return m.Type == "ACK" && len(m.Users) > 0 })
	if len(u.Users) != 1 || u.Users[0].ID != ids[addressee] || u.Users[0].Relation != "outgoing" {
		t.Fatalf("search = %+v, want %d as outgoing", u.Users, ids[addressee])
	}

	sendTest(t, c, protocol.Control{Type: "SEND_FRIENDSHIP_REQUEST", Username: "missing_" + base})
	if e := awaitControl(t, c, "ERROR", func(m protocol.Control) bool { return m.Type == "ERROR" }); e.Error != accounts.ErrNotFound.Error() {
		t.Fatalf("unknown user error = %q, want %q", e.Error, accounts.ErrNotFound.Error())
	}
}

// TestIntegrationAdminModeration: an admin may kick plain members, but not the
// owner or another admin; a plain member may not kick anyone.
func TestIntegrationAdminModeration(t *testing.T) {
	store, db := testStore(t)
	const password = "integration-password-123"
	base := fmt.Sprintf("am_%d", time.Now().UnixNano())
	names := []string{base + "_o", base + "_a", base + "_b", base + "_m"} // owner, admin, admin2, member

	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		for _, n := range names {
			if _, err := db.ExecContext(ctx, "DELETE FROM users WHERE LOWER(username) = LOWER($1)", n); err != nil {
				t.Logf("cleanup user %s: %v", n, err)
			}
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	ids := make([]int, len(names))
	for i, n := range names {
		if err := store.Register(ctx, n, n+"@example.test", password); err != nil {
			t.Fatalf("Register %s: %v", n, err)
		}
		u, err := store.Authenticate(ctx, n, password)
		if err != nil {
			t.Fatalf("Authenticate %s: %v", n, err)
		}
		ids[i] = u.ID
	}
	owner, admin, admin2, member := ids[0], ids[1], ids[2], ids[3]

	ss := servers.NewStore(db)
	srv, err := ss.CreateServer(ctx, owner, base)
	if err != nil {
		t.Fatalf("CreateServer: %v", err)
	}
	for _, id := range ids[1:] {
		if _, err := ss.JoinServer(ctx, id, srv.InviteCode); err != nil {
			t.Fatalf("JoinServer: %v", err)
		}
	}
	for _, id := range []int{admin, admin2} {
		if err := ss.MakeAdmin(ctx, owner, srv.ID, id); err != nil {
			t.Fatalf("MakeAdmin: %v", err)
		}
	}

	if err := ss.KickUser(ctx, member, srv.ID, admin); err == nil {
		t.Fatal("plain member kicked an admin")
	}
	if err := ss.KickUser(ctx, admin, srv.ID, owner); err == nil {
		t.Fatal("admin kicked the owner")
	}
	if err := ss.BanUser(ctx, admin, srv.ID, admin2); err == nil {
		t.Fatal("admin banned another admin")
	}
	if err := ss.BanUser(ctx, admin, srv.ID, member); err != nil {
		t.Fatalf("admin could not ban a member: %v", err)
	}

	// Banned members show up for admins only, and admins may unban them.
	if err := ss.UnbanUser(ctx, member, srv.ID, member); !errors.Is(err, servers.ErrForbidden) {
		t.Fatalf("banned member unban-self error = %v, want ErrForbidden", err)
	}
	list, err := ss.ListMembers(ctx, srv.ID, admin)
	if err != nil {
		t.Fatalf("ListMembers: %v", err)
	}
	var sawBanned bool
	for _, m := range list {
		if m.ID == member && m.Banned {
			sawBanned = true
		}
	}
	if !sawBanned {
		t.Fatal("admin does not see the banned member")
	}
	if err := ss.UnbanUser(ctx, admin, srv.ID, member); err != nil {
		t.Fatalf("admin could not unban: %v", err)
	}
	if err := ss.UnbanUser(ctx, admin, srv.ID, member); !errors.Is(err, servers.ErrNotFound) {
		t.Fatalf("second unban error = %v, want ErrNotFound", err)
	}

	// Rename and new channels: owner and admins only; delete: owner only.
	if _, err := ss.UpdateServer(ctx, admin, srv.ID, base+"_renamed"); err != nil {
		t.Fatalf("admin could not rename: %v", err)
	}
	if _, err := ss.CreateChannel(ctx, "from-admin", 2, srv.ID, admin); err != nil {
		t.Fatalf("admin could not add a channel: %v", err)
	}
	if _, err := ss.UpdateServer(ctx, member, srv.ID, base+"_hack"); !errors.Is(err, servers.ErrForbidden) {
		t.Fatalf("member rename error = %v, want ErrForbidden", err)
	}
	if _, err := ss.CreateChannel(ctx, "from-member", 2, srv.ID, member); !errors.Is(err, servers.ErrForbidden) {
		t.Fatalf("member channel error = %v, want ErrForbidden", err)
	}
	if err := ss.DeleteServer(ctx, admin, srv.ID); err == nil {
		t.Fatal("admin deleted the server")
	}
}
