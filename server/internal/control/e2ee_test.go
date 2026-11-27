package control_test

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
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

// ask sends msg and returns its reply, skipping pushes.
func ask(t *testing.T, c net.Conn, msg protocol.Control) protocol.Control {
	t.Helper()
	sendTest(t, c, msg)
	return awaitControl(t, c, "ERROR", func(m protocol.Control) bool { return m.Type == "ACK" || m.Type == "ERROR" })
}

func wantError(t *testing.T, c net.Conn, msg protocol.Control, want string) {
	t.Helper()
	if got := ask(t, c, msg); got.Type != "ERROR" || got.Error != want {
		t.Fatalf("%s = %s %q, want error %q", msg.Type, got.Type, got.Error, want)
	}
}

func wantACK(t *testing.T, c net.Conn, msg protocol.Control) protocol.Control {
	t.Helper()
	got := ask(t, c, msg)
	if got.Type != "ACK" {
		t.Fatalf("%s = %s %q", msg.Type, got.Type, got.Error)
	}
	return got
}

func TestIntegrationAccountKeys(t *testing.T) {
	store, db := testStore(t)
	name := fmt.Sprintf("ak_%d", time.Now().UnixNano())
	legacy := name + "_l"
	t.Cleanup(func() {
		_, _ = db.Exec(`DELETE FROM users WHERE username IN ($1, $2)`, name, legacy)
	})
	// Every connection shares one address, so each phase gets its own server and
	// with it a fresh per-IP budget.
	dial := func() net.Conn {
		ltcp, _ := net.Listen("tcp", "127.0.0.1:0")
		ludp, _ := net.ListenPacket("udp", "127.0.0.1:0")
		t.Cleanup(func() { ltcp.Close(); ludp.Close() })
		serveTest(t, ltcp, ludp, store, servers.NewStore(db), friends.NewStore(db), messages.NewStore(db))
		c, err := dialTLS(ltcp.Addr().String())
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { c.Close() })
		return c
	}
	salt, key, recovery := hash("s1")[:16], hash("a1"), hash("r1")

	c := dial()
	wantError(t, c, protocol.Control{Type: "REGISTER", Username: name, Email: name + "@x.test", Password: "password123"},
		"password registration is not supported: update the client")
	wantACK(t, c, protocol.Control{Type: "REGISTER", Username: name, Email: name + "@x.test", Salt: salt, AuthKey: key})
	pre := wantACK(t, c, protocol.Control{Type: "PRELOGIN", Identifier: strings.ToUpper(name)})
	if pre.AuthScheme == nil || *pre.AuthScheme != 1 || string(pre.Salt) != string(salt) {
		t.Fatalf("PRELOGIN = %+v", pre)
	}
	unknown := wantACK(t, c, protocol.Control{Type: "PRELOGIN", Identifier: "  Nobody_" + name})
	again := wantACK(t, c, protocol.Control{Type: "PRELOGIN", Identifier: "nobody_" + name})
	if *unknown.AuthScheme != 1 || len(unknown.Salt) != 16 || string(unknown.Salt) != string(again.Salt) {
		t.Fatalf("unknown PRELOGIN = %+v then %+v", unknown, again)
	}

	login := wantACK(t, c, protocol.Control{Type: "LOGIN", Identifier: name, AuthKey: key})
	if login.UserID == 0 || *login.AuthScheme != 1 || login.AccountKeys != nil {
		t.Fatalf("LOGIN = %+v", login)
	}
	for _, typ := range []string{"HELLO", "LIST_SERVERS", "UPDATE_PASSWORD", "GET_USER_KEYS", "DELETE_USER"} {
		wantError(t, c, protocol.Control{Type: typ, SSRC: 1}, "account keys required")
	}
	keys, _ := testAccountKeys(login.UserID)
	bad := *keys
	bad.KeySig = filled(64)
	wantError(t, c, protocol.Control{Type: "SET_KEYS", AccountKeys: &bad, RecoveryAuthKey: recovery}, "invalid account keys")
	set := wantACK(t, c, protocol.Control{Type: "SET_KEYS", AccountKeys: keys, RecoveryAuthKey: recovery})
	if set.AccountKeys.KeyID == 0 || set.AccountKeys.AKRecovery != nil || set.DeviceToken == "" {
		t.Fatalf("SET_KEYS = %+v", set)
	}
	wantError(t, c, protocol.Control{Type: "SET_KEYS", AccountKeys: keys, RecoveryAuthKey: recovery}, "account keys already set")
	wantACK(t, c, protocol.Control{Type: "LIST_SERVERS"})

	// Password change: old key checked, all three replaced.
	salt2, key2, ak2 := hash("s2")[:16], hash("a2"), filled(72)
	change := protocol.Control{Type: "UPDATE_PASSWORD", AuthKey: hash("wrong"), NewSalt: salt2, NewAuthKey: key2,
		AccountKeys: &e2ee.AccountKeys{AKPassword: ak2}}
	wantError(t, c, change, "invalid credentials")
	change.AuthKey = key
	if wantACK(t, c, change).DeviceToken == "" {
		t.Fatal("no device token after password change")
	}
	got, err := store.CurrentKeys(context.Background(), login.UserID)
	if err != nil || string(got.AKPassword) != string(ak2) {
		t.Fatalf("akPassword not replaced: %v", err)
	}

	// Recovery: uniform refusal, then new password and code, same identity.
	r := dial()
	wantError(t, r, protocol.Control{Type: "RECOVER_BEGIN", Identifier: name, RecoveryAuthKey: hash("bad")}, uniformRefusal)
	wantError(t, r, protocol.Control{Type: "RECOVER_BEGIN", Identifier: "nobody_" + name, RecoveryAuthKey: recovery}, uniformRefusal)
	begin := wantACK(t, r, protocol.Control{Type: "RECOVER_BEGIN", Identifier: name, RecoveryAuthKey: recovery})
	if begin.UserID != login.UserID || len(begin.AccountKeys.AKRecovery) != 72 {
		t.Fatalf("RECOVER_BEGIN = %+v", begin)
	}

	// RECOVER_FINISH and the reset, through the store.
	ctx := context.Background()
	salt3, key3, recovery3 := hash("s3")[:16], hash("a3"), hash("r3")
	next := accounts.Recovery{Salt: salt3, AuthKey: key3, RecoveryAuthKey: recovery3,
		Keys: &e2ee.AccountKeys{AKPassword: filled(72), AKRecovery: filled(72)}}
	if _, err := store.RecoverFinish(ctx, name, hash("bad"), next); err != accounts.ErrBadCredentials {
		t.Fatalf("wrong recovery key: %v", err)
	}
	if _, err := store.RecoverFinish(ctx, name, recovery, next); err != nil {
		t.Fatal(err)
	}
	if _, err := store.RecoverFinish(ctx, name, recovery, next); err != accounts.ErrBadCredentials {
		t.Fatalf("old recovery code: %v", err)
	}
	if _, err := store.Authenticate(ctx, name, accounts.Secret{AuthKey: key2}); err != accounts.ErrBadCredentials {
		t.Fatalf("old password: %v", err)
	}
	if _, err := store.Authenticate(ctx, name, accounts.Secret{AuthKey: key3}); err != nil {
		t.Fatal(err)
	}
	after, _ := store.CurrentKeys(ctx, login.UserID)
	if after.KeyID != got.KeyID || string(after.IdentityPub) != string(got.IdentityPub) {
		t.Fatal("recovery changed the identity")
	}

	// Operator reset: scheme 0 with a temporary password, keys retired.
	password, err := store.Reset(ctx, strings.ToUpper(name))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Authenticate(ctx, name, accounts.Secret{AuthKey: key3}); err != accounts.ErrBadCredentials {
		t.Fatalf("auth key after reset: %v", err)
	}
	if _, err := store.CurrentKeys(ctx, login.UserID); err != accounts.ErrNoKeys {
		t.Fatalf("keys after reset: %v", err)
	}
	old, err := store.UserKeys(ctx, login.UserID, login.UserID, got.KeyID)
	if err != nil || !old.Retired {
		t.Fatalf("retired key = %+v, %v", old, err)
	}
	if _, err := store.Authenticate(ctx, name, accounts.Secret{Password: password}); err != nil {
		t.Fatal(err)
	}

	// Upgrade of a legacy session: salt and authKey are required and replace
	// the hash.
	u := dial()
	up := wantACK(t, u, protocol.Control{Type: "LOGIN", Identifier: name, Password: password})
	if up.AuthScheme == nil || *up.AuthScheme != 0 || up.AccountKeys != nil {
		t.Fatalf("legacy LOGIN = %+v", up)
	}
	keys, _ = testAccountKeys(up.UserID)
	upgrade := protocol.Control{Type: "SET_KEYS", AccountKeys: keys, RecoveryAuthKey: recovery}
	wantError(t, u, upgrade, "invalid account keys")
	upgrade.Salt, upgrade.AuthKey = salt, key
	if wantACK(t, u, upgrade).AccountKeys.KeyID == got.KeyID {
		t.Fatal("keyId reused")
	}
	if _, err := store.Authenticate(ctx, name, accounts.Secret{Password: password}); err != accounts.ErrBadCredentials {
		t.Fatalf("password after upgrade: %v", err)
	}
	if _, err := store.Authenticate(ctx, name, accounts.Secret{AuthKey: key}); err != nil {
		t.Fatal(err)
	}

	// A session that logged in before a reset cannot set keys.
	if _, err := store.Reset(ctx, name); err != nil {
		t.Fatal(err)
	}
	keys, _ = testAccountKeys(up.UserID)
	if _, _, err := store.SetKeys(ctx, up.UserID, "stale credential", keys, recovery, salt, key); err != accounts.ErrBadCredentials {
		t.Fatalf("stale session SET_KEYS: %v", err)
	}
}

const uniformRefusal = "invalid credentials, or too many failed attempts: try again later"

// keysOf fetches GET_CONVERSATION_KEYS over the wire.
func keysOf(t *testing.T, c net.Conn, address protocol.Control) messages.ConversationKeys {
	t.Helper()
	address.Type = "GET_CONVERSATION_KEYS"
	sendTest(t, c, address)
	c.SetReadDeadline(time.Now().Add(10 * time.Second))
	defer c.SetReadDeadline(time.Time{})
	for {
		payload, err := protocol.ReadFrame(c)
		if err != nil {
			t.Fatal(err)
		}
		var head protocol.Control
		_ = json.Unmarshal(payload, &head) // members differs from the roster field
		if head.Type == "ERROR" {
			t.Fatalf("GET_CONVERSATION_KEYS: %s", head.Error)
		}
		if strings.Contains(string(payload), `"moreEpochs"`) {
			var k messages.ConversationKeys
			if err := json.Unmarshal(payload, &k); err != nil {
				t.Fatal(err)
			}
			return k
		}
	}
}

func createRequest(address protocol.Control, e messages.NewEpoch) protocol.Control {
	address.Type, address.Epoch, address.KeyCommit = "CREATE_EPOCH", e.Epoch, e.KeyCommit
	address.CreatorKeyID, address.EpochSig, address.Wraps = e.CreatorKeyID, e.EpochSig, e.Wraps
	return address
}

func identityOf(userID int) ed25519.PrivateKey {
	k, _ := testIDs.Load(userID)
	return k.(ed25519.PrivateKey)
}

func TestIntegrationEpochsAndMessages(t *testing.T) {
	f := setupChatFixture(t)
	owner, outsider := f.conns[0], f.conns[2]
	addr := protocol.Control{ServerID: f.srv1.ID, ChannelID: f.ch1.ID}

	k := keysOf(t, owner, addr)
	if k.CurrentEpoch != 0 || k.Stale || len(k.Members) != 2 || k.ConversationID == 0 {
		t.Fatalf("empty keys = %+v", k)
	}
	denied := addr
	denied.Type = "GET_CONVERSATION_KEYS"
	wantError(t, outsider, denied, "server channel not found")

	// Message before any epoch.
	send := func(epoch uint32, msgID []byte, revision uint32) protocol.Control {
		return protocol.Control{Type: "SEND_SERVER_MESSAGE", ServerID: f.srv1.ID, ChannelID: f.ch1.ID,
			E2EE: testMessage(identityOf(f.ids[0]), k.ConversationID, f.ids[0], k.Members[0].KeyID, epoch, revision, msgID, "hi")}
	}
	wantError(t, owner, send(1, filled(16), 0), "stale epoch")

	// CREATE_EPOCH must cover exactly the keyed members with good signatures.
	a := messages.Address{ServerID: f.srv1.ID, Channel: messages.ChannelID(f.ch1.ID)}
	good := testEpoch(f.ids[0], a, &k)
	for name, change := range map[string]func(e *messages.NewEpoch){
		"missing wrap": func(e *messages.NewEpoch) { e.Wraps = e.Wraps[:1] },
		"repeated":     func(e *messages.NewEpoch) { e.Wraps = []e2ee.NewWrap{e.Wraps[0], e.Wraps[0]} },
		"wrong epoch":  func(e *messages.NewEpoch) { e.Epoch = 2 },
		"bad epochSig": func(e *messages.NewEpoch) { e.EpochSig = filled(64) },
		"bad wrapSig": func(e *messages.NewEpoch) {
			e.Wraps = append([]e2ee.NewWrap{}, e.Wraps...)
			e.Wraps[1].WrapSig = filled(64)
		},
	} {
		e := good
		change(&e)
		if got := ask(t, owner, createRequest(addr, e)); got.Error != "epoch conflict" {
			t.Fatalf("%s = %+v", name, got)
		}
	}
	// Concurrent creators: exactly one wins.
	store := messages.NewStore(f.db)
	results := make(chan error, 2)
	for _, id := range f.ids[:2] {
		e := testEpoch(id, a, &k)
		go func(id int) {
			_, err := store.CreateEpoch(f.ctx, id, a, e)
			results <- err
		}(id)
	}
	first, second := <-results, <-results
	if (first == nil) == (second == nil) || (first != messages.ErrEpochConflict && second != messages.ErrEpochConflict) {
		t.Fatalf("concurrent create = %v, %v", first, second)
	}
	// A healthy epoch below the message limit cannot be rotated.
	k = keysOf(t, owner, addr)
	wantError(t, owner, createRequest(addr, testEpoch(f.ids[0], a, &k)), "epoch conflict")

	// Message checks.
	msgID := filled(16)
	badSig := send(1, msgID, 0)
	badSig.E2EE.MsgSig = filled(64)
	wantError(t, owner, badSig, "invalid encrypted message")
	badBox := send(1, msgID, 0)
	badBox.E2EE.Box = badBox.E2EE.Box[:200]
	wantError(t, owner, badBox, "invalid encrypted message")
	wantError(t, owner, send(1, msgID, 1), "invalid message revision")
	plain := send(1, msgID, 0)
	plain.ServerContent = "leak"
	raw, _ := json.Marshal(plain)
	_ = protocol.WriteFrame(owner, raw)
	awaitControl(t, owner, "ERROR", func(m protocol.Control) bool { return m.Error == "plaintext content refused" })
	sent := wantACK(t, owner, send(1, msgID, 0))
	if sent.ServerInfo.E2EE == nil || sent.ServerInfo.Content != "" {
		t.Fatalf("ACK = %+v", sent.ServerInfo)
	}
	// The per-user send burst is 5: refill before the next sends.
	time.Sleep(3 * time.Second)
	wantError(t, owner, send(1, msgID, 0), "duplicate message id")
	update := func(msgID []byte, revision uint32) protocol.Control {
		m := send(1, msgID, revision)
		m.Type, m.ServerMessageID = "UPDATE_SERVER_MESSAGE", protocol.MessageID(sent.ServerInfo.ID)
		return m
	}
	wantError(t, owner, update(filled(16), 1), "invalid message revision")
	wantError(t, owner, update(msgID, 2), "invalid message revision")
	wantACK(t, owner, update(msgID, 1))

	// Sharing: a newcomer lacks a wrap, which does not make the epoch stale.
	if _, err := f.serverStore.JoinServer(f.ctx, f.ids[2], f.srv1.InviteCode); err != nil {
		t.Fatal(err)
	}
	k = keysOf(t, owner, addr)
	if k.Stale || len(k.Missing) != 1 || k.Missing[0].UserID != f.ids[2] || len(k.Epochs[0].Wraps) != 1 {
		t.Fatalf("keys after join = %+v", k)
	}
	share := func(sender int, to e2ee.UserKey) protocol.Control {
		w := testWrap(identityOf(sender), k.ConversationID, testDesc(sender, a), 1, sender, keyOf(k, sender), to, k.Epochs[0].KeyCommit)
		w.Epoch = 1
		m := addr
		m.Type, m.SenderKeyID, m.Shares = "SHARE_KEYS", keyOf(k, sender), []e2ee.NewWrap{w}
		return m
	}
	newcomer := memberOf(k, f.ids[2])
	// The newcomer holds no wrap, so cannot share.
	wantError(t, outsider, share(f.ids[2], memberOf(k, f.ids[1])), "invalid key share")
	forged := share(f.ids[0], newcomer)
	forged.Shares[0].WrapSig = filled(64)
	wantError(t, owner, forged, "invalid key share")
	ok := share(f.ids[0], newcomer)
	wantACK(t, owner, ok)
	push := awaitControl(t, outsider, "KEYS_CHANGED", func(m protocol.Control) bool { return m.Type == "KEYS_CHANGED" })
	if push.ConversationID != k.ConversationID || push.ServerID != f.srv1.ID || push.ChannelID != f.ch1.ID {
		t.Fatalf("push = %+v", push)
	}
	wantACK(t, owner, ok) // duplicate ignored
	if got := keysOf(t, outsider, addr); len(got.Epochs[0].Wraps) != 1 || len(got.Missing) != 0 {
		t.Fatalf("newcomer keys = %+v", got)
	}

	// Ban, then kick: each makes an epoch stale; rotation repairs it.
	for i, remove := range []func() error{
		func() error { return f.serverStore.BanUser(f.ctx, f.ids[0], f.srv1.ID, f.ids[2]) },
		func() error { return f.serverStore.KickUser(f.ctx, f.ids[0], f.srv1.ID, f.ids[1]) },
		func() error { _, err := f.db.Exec(`DELETE FROM users WHERE id = $1`, f.ids[1]); return err },
	} {
		if i == 2 { // rejoin and rotate, so deletion has someone to remove
			if _, err := f.serverStore.JoinServer(f.ctx, f.ids[1], f.srv1.InviteCode); err != nil {
				t.Fatal(err)
			}
			k = keysOf(t, owner, addr)
			// Rejoining does not stale the epoch made without them. Sharing
			// records them as a recipient.
			if k.Stale {
				t.Fatal("a newcomer made the epoch stale")
			}
			w := testWrap(identityOf(f.ids[0]), k.ConversationID, testDesc(f.ids[0], a), k.CurrentEpoch, f.ids[0], keyOf(k, f.ids[0]),
				memberOf(k, f.ids[1]), k.Epochs[0].KeyCommit)
			w.Epoch = k.CurrentEpoch
			if _, _, err := store.ShareKeys(f.ctx, f.ids[0], a, keyOf(k, f.ids[0]), []e2ee.NewWrap{w}); err != nil {
				t.Fatal(err)
			}
		}
		if err := remove(); err != nil {
			t.Fatal(err)
		}
		k = keysOf(t, owner, addr)
		if !k.Stale {
			t.Fatalf("removal %d: epoch not stale", i)
		}
		time.Sleep(time.Second)
		wantError(t, owner, send(k.CurrentEpoch, filled(16), 0), "stale epoch")
		wantACK(t, owner, createRequest(addr, testEpoch(f.ids[0], a, &k)))
		k = keysOf(t, owner, addr)
		if k.Stale {
			t.Fatalf("removal %d: still stale after rotation", i)
		}
	}
	time.Sleep(2 * time.Second)
	wantError(t, owner, send(1, filled(16), 0), "stale epoch")

	// A deleted account's public key stays available through the epochs.
	var keyID int64
	if err := f.db.QueryRow(`SELECT id FROM user_keys WHERE user_id = $1`, f.ids[1]).Scan(&keyID); err != nil {
		t.Fatal(err)
	}
	if !wantACK(t, owner, protocol.Control{Type: "GET_USER_KEYS", UserID: f.ids[1], KeyID: keyID}).UserKeys.Retired {
		t.Fatal("deleted user's key not retired")
	}
}

func memberOf(k messages.ConversationKeys, userID int) e2ee.UserKey {
	for _, m := range k.Members {
		if m.UserID == userID {
			return m
		}
	}
	return e2ee.UserKey{}
}

func keyOf(k messages.ConversationKeys, userID int) int64 { return memberOf(k, userID).KeyID }

func TestIntegrationUserKeysAndPages(t *testing.T) {
	f := setupChatFixture(t)
	owner, member, outsider := f.conns[0], f.conns[1], f.conns[2]

	got := wantACK(t, owner, protocol.Control{Type: "GET_USER_KEYS", UserID: f.ids[1]})
	if got.UserKeys.Retired || got.UserKeys.UserID != f.ids[1] {
		t.Fatalf("userKeys = %+v", got.UserKeys)
	}
	wantError(t, outsider, protocol.Control{Type: "GET_USER_KEYS", UserID: f.ids[1]}, "user keys not found")
	wantError(t, owner, protocol.Control{Type: "GET_USER_KEYS", UserID: f.ids[1], KeyID: got.UserKeys.KeyID + 1000}, "user keys not found")
	wantACK(t, outsider, protocol.Control{Type: "GET_USER_KEYS", UserID: f.ids[2]})

	// Large boxes: pages stay under 512 KiB and say there is more.
	peer := strconv.Itoa(f.ids[1])
	first := seal(t, owner, protocol.Control{Type: "SEND_DIRECT_MESSAGE", DirectRecipientID: peer,
		DirectContent: strings.Repeat("a", 16000)})
	// The peer reads its pushes: unread, they fill the socket buffer and the
	// server drops the slow connection.
	send := func(msg protocol.Control) {
		wantACK(t, owner, msg)
		awaitControl(t, member, "DIRECT_MESSAGE", func(m protocol.Control) bool { return m.Type == "DIRECT_MESSAGE" })
	}
	send(first)
	for i := 0; i < 29; i++ {
		send(seal(t, owner, protocol.Control{Type: "SEND_DIRECT_MESSAGE",
			DirectRecipientID: peer, DirectContent: strings.Repeat("b", 16000)}))
	}
	page := wantACK(t, member, protocol.Control{Type: "RECIEVE_DIRECT_MESSAGE", DirectRecipientID: strconv.Itoa(f.ids[0])})
	size, _ := json.Marshal(page.DirectMessages)
	if page.HasMore == nil || !*page.HasMore || len(size) > 512<<10 || len(page.DirectMessages) < 10 {
		t.Fatalf("page of %d messages, %d bytes", len(page.DirectMessages), len(size))
	}
	older := wantACK(t, member, protocol.Control{Type: "RECIEVE_DIRECT_MESSAGE", DirectRecipientID: strconv.Itoa(f.ids[0]),
		DMBeforeID: page.DirectMessages[len(page.DirectMessages)-1].ID})
	if *older.HasMore || len(older.DirectMessages)+len(page.DirectMessages) != 31 { // 30 and the seeded one
		t.Fatalf("older page has %d messages", len(older.DirectMessages))
	}
	list := wantACK(t, member, protocol.Control{Type: "LIST_DIRECT_CONVERSATIONS"})
	if *list.HasMore || list.DirectConversations[0].E2EE == nil || list.DirectConversations[0].AuthorID != f.ids[0] {
		t.Fatalf("conversation list = %+v", list)
	}
	empty := wantACK(t, member, protocol.Control{Type: "LIST_DIRECT_CONVERSATIONS",
		BeforeConversationID: list.DirectConversations[0].ChannelID})
	if len(empty.DirectConversations) != 0 {
		t.Fatalf("page after the last conversation = %+v", empty.DirectConversations)
	}

	// Deleting removes the box; the id stays taken.
	id := page.DirectMessages[0].ID
	wantACK(t, owner, protocol.Control{Type: "DELETE_DIRECT_MESSAGE", DirectMessageID: id})
	var box []byte
	if err := f.db.QueryRow(`SELECT e2ee_box FROM conversation_messages WHERE id = $1`, id).Scan(&box); err != nil || box != nil {
		t.Fatalf("box after delete = %d bytes, %v", len(box), err)
	}

	// A deleted peer: plaintext history with a NULL author still lists.
	if _, err := f.db.Exec(`DELETE FROM users WHERE id = $1`, f.ids[0]); err != nil {
		t.Fatal(err)
	}
	history := wantACK(t, member, protocol.Control{Type: "RECIEVE_DIRECT_MESSAGE",
		DirectRecipientID: strconv.Itoa(f.ids[0]), DMBeforeID: older.DirectMessages[len(older.DirectMessages)-2].ID})
	if len(history.DirectMessages) != 1 || history.DirectMessages[0].AuthorID != 0 {
		t.Fatalf("plaintext history of a deleted author = %+v", history.DirectMessages)
	}
	// Their keys stay readable to the other side of the conversation.
	old := wantACK(t, member, protocol.Control{Type: "GET_USER_KEYS", UserID: f.ids[0], KeyID: first.E2EE.SenderKeyID})
	if !old.UserKeys.Retired {
		t.Fatal("deleted user's key not retired")
	}
}
