package control_test

import (
	"context"
	"errors"
	"fmt"
	"net"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/e2ee"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// awaitClosed fails unless the server ends the connection within limit.
func awaitClosed(t *testing.T, c net.Conn, limit time.Duration) {
	t.Helper()
	c.SetReadDeadline(time.Now().Add(limit))
	for {
		if _, err := protocol.ReadFrame(c); err != nil {
			var timeout net.Error
			if errors.As(err, &timeout) && timeout.Timeout() {
				t.Fatal("session not ended")
			}
			return
		}
	}
}

func TestIntegrationSessionsRecoveryAndDelete(t *testing.T) {
	store, db := testStore(t)
	ctx := context.Background()
	name := fmt.Sprintf("se_%d", time.Now().UnixNano())
	t.Cleanup(func() { _, _ = db.Exec(`DELETE FROM users WHERE username = $1`, name) })
	// Each server has its own per-IP budget and TLS key.
	server := func() func() net.Conn {
		ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
		ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
		t.Cleanup(func() { ltcp.Close(); ludp.Close() })
		serveTest(t, ltcp, ludp, store, servers.NewStore(db), friends.NewStore(db), messages.NewStore(db))
		return func() net.Conn {
			c, err := dialTLS(ltcp.Addr().String())
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { c.Close() })
			return c
		}
	}
	first, second := server(), server()

	// The made-up salt comes from the database, not from one process or TLS key.
	unknown := protocol.Control{Type: "PRELOGIN", Identifier: "nobody_" + name}
	a, b := wantACK(t, first(), unknown), wantACK(t, second(), unknown)
	if len(a.Salt) != 16 || string(a.Salt) != string(b.Salt) {
		t.Fatalf("fake salt differs between servers: %x, %x", a.Salt, b.Salt)
	}

	if err := testRegister(ctx, store, name, name+"@x.test", "password-1"); err != nil {
		t.Fatal(err)
	}
	login := func(c net.Conn, key []byte) protocol.Control {
		return wantACK(t, c, protocol.Control{Type: "LOGIN", Identifier: name, AuthKey: key})
	}
	key, recovery := hash("auth", "password-1"), hash("recovery", name)
	one, two := first(), first()
	id := login(one, key).UserID
	keys, _ := testAccountKeys(id)
	wantACK(t, one, protocol.Control{Type: "SET_KEYS", AccountKeys: keys, RecoveryAuthKey: recovery})
	login(two, key)

	// A password change ends the other sessions only.
	key2 := hash("a2")
	wantACK(t, one, protocol.Control{Type: "UPDATE_PASSWORD", AuthKey: key, NewSalt: hash("s2")[:16], NewAuthKey: key2,
		AccountKeys: &e2ee.AccountKeys{AKPassword: filled(72)}})
	awaitClosed(t, two, 5*time.Second)
	wantACK(t, one, protocol.Control{Type: "LIST_SERVERS"})

	// Recovery has no per-account budget, and ends the sessions.
	r := second()
	for i := 0; i < 6; i++ {
		wantError(t, r, protocol.Control{Type: "RECOVER_BEGIN", Identifier: name, RecoveryAuthKey: filled(32)}, uniformRefusal)
	}
	wantACK(t, r, protocol.Control{Type: "RECOVER_BEGIN", Identifier: name, RecoveryAuthKey: recovery})
	third := first()
	login(third, key2)
	key3 := hash("a3")
	// Same server as the session, as in production.
	r = first()
	wantACK(t, r, protocol.Control{Type: "RECOVER_FINISH", Identifier: name, RecoveryAuthKey: recovery,
		NewSalt: hash("s3")[:16], NewAuthKey: key3, NewRecoveryAuthKey: hash("r3"),
		AccountKeys: &e2ee.AccountKeys{AKPassword: filled(72), AKRecovery: filled(72)}})
	awaitClosed(t, third, 5*time.Second)
	awaitClosed(t, one, 5*time.Second)

	// DELETE_USER needs the current auth key.
	d := second()
	login(d, key3)
	wantError(t, d, protocol.Control{Type: "DELETE_USER"}, "invalid credentials")
	wantError(t, d, protocol.Control{Type: "DELETE_USER", AuthKey: key2}, "invalid credentials")

	// A reset by another process is noticed by the recheck.
	password, err := store.Reset(ctx, name)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(password, "FEATHER-RESET-") || len(password) > 72 {
		t.Fatalf("temporary password %q", password)
	}
	awaitClosed(t, d, 30*time.Second)

	u := second()
	up := wantACK(t, u, protocol.Control{Type: "LOGIN", Identifier: name, Password: password})
	keys, _ = testAccountKeys(up.UserID)
	wantACK(t, u, protocol.Control{Type: "SET_KEYS", AccountKeys: keys, RecoveryAuthKey: recovery,
		Salt: hash("s4")[:16], AuthKey: key})
	wantACK(t, u, protocol.Control{Type: "DELETE_USER", AuthKey: key})
	if _, err := store.StoredCredential(ctx, id); err != accounts.ErrNotFound {
		t.Fatalf("account after delete: %v", err)
	}
}

func TestIntegrationKeyRequestGates(t *testing.T) {
	f := setupChatFixture(t)
	owner, outsider := f.conns[0], f.conns[2]
	store := messages.NewStore(f.db)
	toOwner := messages.Address{PeerID: f.ids[0]}

	// A stranger gets no DM conversation; a friend does.
	if _, err := store.ConversationKeys(f.ctx, f.ids[2], toOwner, 0); err != messages.ErrNotFound {
		t.Fatalf("stranger: %v", err)
	}
	for _, typ := range []string{"GET_CONVERSATION_KEYS", "CREATE_EPOCH", "REJECT_WRAP"} {
		wantError(t, outsider, protocol.Control{Type: typ, DirectRecipientID: strconv.Itoa(f.ids[0])},
			messages.ErrNotFound.Error())
	}
	if _, _, err := store.ShareKeys(f.ctx, f.ids[2], toOwner, 1, make([]e2ee.NewWrap, 1)); err != messages.ErrNotFound {
		t.Fatalf("stranger share: %v", err)
	}
	if _, err := f.db.Exec(`INSERT INTO friendships (requester_id, addressee_id, status) VALUES ($1, $2, 'ACCEPTED')`,
		f.ids[0], f.ids[2]); err != nil {
		t.Fatal(err)
	}
	k, err := store.ConversationKeys(f.ctx, f.ids[2], toOwner, 0)
	if err != nil {
		t.Fatalf("friend: %v", err)
	}
	// An existing message keeps the DM reachable.
	if _, err := store.SendDM(f.ctx, f.ids[0], f.ids[2], 0, "old"); err != nil {
		t.Fatal(err)
	}
	if _, err := f.db.Exec(`DELETE FROM friendships WHERE addressee_id = $1`, f.ids[2]); err != nil {
		t.Fatal(err)
	}
	if _, err := store.ConversationKeys(f.ctx, f.ids[2], toOwner, 0); err != nil {
		t.Fatalf("after a message: %v", err)
	}

	// The read does not wait for the conversation lock.
	tx, err := f.db.Begin()
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()
	if _, err := tx.Exec(`SELECT 1 FROM conversations WHERE id = $1 FOR UPDATE`, k.ConversationID); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(f.ctx, 3*time.Second)
	defer cancel()
	if _, err := store.ConversationKeys(ctx, f.ids[2], toOwner, 0); err != nil {
		t.Fatalf("read under a lock: %v", err)
	}
	_ = tx.Rollback()

	// Editing and deleting the same message do not deadlock.
	dm := messages.Address{PeerID: f.ids[1]}
	dk, err := store.ConversationKeys(f.ctx, f.ids[0], dm, 0)
	if err != nil {
		t.Fatal(err)
	}
	e := testEpoch(f.ids[0], dm, dk)
	if _, err := store.CreateEpoch(f.ctx, f.ids[0], dm, e); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 40; i++ {
		msgID := filled(16)
		m := func(revision uint32) *e2ee.Message {
			return testMessage(identityOf(f.ids[0]), dk.ConversationID, f.ids[0], e.CreatorKeyID, 1, revision, msgID, "x")
		}
		sent, err := store.SendEncryptedDM(f.ctx, f.ids[0], f.ids[1], 0, m(0))
		if err != nil {
			t.Fatal(err)
		}
		edit := make(chan error, 1)
		go func(m *e2ee.Message) {
			_, _, err := store.EditDM(f.ctx, f.ids[0], sent.ID, m)
			edit <- err
		}(m(1))
		if _, _, err := store.DeleteDM(f.ctx, f.ids[0], sent.ID); err != nil {
			t.Fatalf("delete %d: %v", i, err)
		}
		if err := <-edit; err != nil && err != messages.ErrMessageNotFound {
			t.Fatalf("edit %d: %v", i, err)
		}
	}

	// All key messages share one budget per user.
	limited := false
	for i := 0; i < 80 && !limited; i++ {
		got := ask(t, owner, protocol.Control{Type: "GET_USER_KEYS", UserID: f.ids[0]})
		limited = got.Error == "too many attempts, try again later"
	}
	if !limited {
		t.Fatal("key requests not limited")
	}
	wantError(t, owner, protocol.Control{Type: "GET_CONVERSATION_KEYS", DirectRecipientID: strconv.Itoa(f.ids[1])},
		"too many attempts, try again later")
}

func TestIntegrationRejectWrap(t *testing.T) {
	f := setupChatFixture(t)
	store := messages.NewStore(f.db)
	a := messages.Address{ServerID: f.srv1.ID, Channel: messages.ChannelID(f.ch1.ID)}
	owner, member, late := f.ids[0], f.ids[1], f.ids[2]
	keys := func(user int) *messages.ConversationKeys {
		k, err := store.ConversationKeys(f.ctx, user, a, 0)
		if err != nil {
			t.Fatal(err)
		}
		return k
	}
	create := func() {
		if _, err := store.CreateEpoch(f.ctx, owner, a, testEpoch(owner, a, keys(owner))); err != nil {
			t.Fatal(err)
		}
	}
	reject := func(user int, epoch uint32, sender int) {
		if _, err := store.RejectWrap(f.ctx, user, a, epoch, sender); err != nil {
			t.Fatalf("reject by %d: %v", user, err)
		}
	}
	create()
	if _, err := f.serverStore.JoinServer(f.ctx, late, f.srv1.InviteCode); err != nil {
		t.Fatal(err)
	}
	share := func(sender int) {
		k := keys(sender)
		w := testWrap(identityOf(sender), k.ConversationID, testDesc(sender, a), 1, sender, keyOf(*k, sender),
			memberOf(*k, late), k.Epochs[0].KeyCommit)
		w.Epoch = 1
		if _, _, err := store.ShareKeys(f.ctx, sender, a, keyOf(*k, sender), []e2ee.NewWrap{w}); err != nil {
			t.Fatalf("share by %d: %v", sender, err)
		}
	}
	share(member)
	share(owner)
	if w := keys(late).Epochs[0].Wraps; len(w) != 2 || w[0].SenderID != owner {
		t.Fatalf("creator not first: %+v", w)
	}

	// Rejecting a sender who is not the creator hides the wrap only.
	reject(late, 1, member)
	if k := keys(late); k.Stale || len(k.Epochs[0].Wraps) != 1 {
		t.Fatalf("after rejecting a sharer: %+v", k)
	}
	share(member) // cannot replace it
	if w := keys(late).Epochs[0].Wraps; len(w) != 1 {
		t.Fatalf("rejected wrap replaced: %+v", w)
	}

	// The creator's wrap: stale, and no longer missing for the creator only.
	reject(late, 1, owner)
	if k := keys(owner); !k.Stale || len(k.Missing) != 0 {
		t.Fatalf("creator view: %+v", k)
	}
	if k := keys(member); len(k.Missing) != 0 {
		t.Fatalf("member view: %+v", k.Missing)
	}
	if k := keys(late); len(k.Epochs[0].Wraps) != 0 || len(k.Missing) != 0 {
		t.Fatalf("rejecter view: %+v", k)
	}
	reject(late, 1, owner) // repeat
	if _, err := store.RejectWrap(f.ctx, late, a, 1, late); err != messages.ErrBadShare {
		t.Fatalf("unknown wrap: %v", err)
	}
	wantError(t, f.conns[2], protocol.Control{Type: "REJECT_WRAP", ServerID: f.srv1.ID, ChannelID: f.ch1.ID, Epoch: 9},
		"invalid key share")

	// One user makes an epoch stale at most once a minute; another still can.
	create()
	reject(late, 2, owner)
	if k := keys(owner); k.Stale || k.CurrentEpoch != 2 {
		t.Fatalf("second rejection within a minute: %+v", k)
	}
	wantACK(t, f.conns[1], protocol.Control{Type: "REJECT_WRAP", ServerID: f.srv1.ID, ChannelID: f.ch1.ID,
		Epoch: 2, SenderID: owner})
	if !keys(owner).Stale {
		t.Fatal("another user's rejection ignored")
	}
}

func TestIntegrationServerMemberCap(t *testing.T) {
	f := setupChatFixture(t)
	prefix := fmt.Sprintf("cap%d_", time.Now().UnixNano()%1e9)
	t.Cleanup(func() { _, _ = f.db.Exec(`DELETE FROM users WHERE username LIKE $1`, prefix+"%") })
	// The fixture's server has two members.
	if _, err := f.db.Exec(`
		WITH u AS (
			INSERT INTO users (username, email, password_hash)
			SELECT $1::text || n, $1::text || n || '@x.test', '' FROM generate_series(1, 998) n RETURNING id
		)
		INSERT INTO server_members (server_id, user_id) SELECT $2, id FROM u`, prefix, f.srv1.ID); err != nil {
		t.Fatal(err)
	}
	wantError(t, f.conns[2], protocol.Control{Type: "JOIN_SERVER", InviteCode: f.srv1.InviteCode}, "server is full")
	if _, err := f.serverStore.JoinServer(f.ctx, f.ids[1], f.srv1.InviteCode); err != nil {
		t.Fatalf("member rejoining a full server: %v", err)
	}
	if err := f.serverStore.KickUser(f.ctx, f.ids[0], f.srv1.ID, f.ids[1]); err != nil {
		t.Fatal(err)
	}
	wantACK(t, f.conns[2], protocol.Control{Type: "JOIN_SERVER", InviteCode: f.srv1.InviteCode})
}
