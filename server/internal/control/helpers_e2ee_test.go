package control_test

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"net"
	"strconv"
	"strings"
	"sync"
	"testing"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/e2ee"
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// The server never opens anything, so test "ciphertext" is the padded text
// itself and wraps are random bytes under real signatures.

var (
	testDB    *sql.DB
	testConns sync.Map // net.Conn -> userID
	testIDs   sync.Map // userID -> ed25519.PrivateKey
)

func hash(parts ...string) []byte {
	sum := sha256.Sum256([]byte(strings.Join(parts, "|")))
	return sum[:]
}

func filled(n int) []byte {
	b := make([]byte, n)
	_, _ = rand.Read(b)
	return b
}

func testSecret(password string) accounts.Secret {
	return accounts.Secret{AuthKey: hash("auth", password)}
}

func testRegister(ctx context.Context, store *accounts.Store, name, email, password string) error {
	return store.Register(ctx, name, email, hash("salt", name)[:16], hash("auth", password))
}

// testAccountKeys builds a SET_KEYS accountKeys object with a fresh identity.
func testAccountKeys(userID int) (*e2ee.AccountKeys, ed25519.PrivateKey) {
	pub, priv, _ := ed25519.GenerateKey(nil)
	encPub := filled(32)
	return &e2ee.AccountKeys{
		IdentityPub: pub, EncPub: encPub,
		KeySig:     ed25519.Sign(priv, e2ee.KeyRecord(userID, pub, encPub)),
		AKPassword: filled(72), AKRecovery: filled(72), Bundle: filled(104),
	}, priv
}

// loginTest logs c in and gives the account keys if it has none.
func loginTest(t *testing.T, c net.Conn, username, password string) {
	t.Helper()
	sendTest(t, c, protocol.Control{Type: "LOGIN", Identifier: username, AuthKey: hash("auth", password)})
	// LOGIN ACK carries the username; waiting keeps REGISTER/LOGIN ordered across clients.
	ack := awaitControl(t, c, "LOGIN ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.Name != "" })
	if ack.AccountKeys == nil {
		keys, priv := testAccountKeys(ack.UserID)
		sendTest(t, c, protocol.Control{Type: "SET_KEYS", AccountKeys: keys, RecoveryAuthKey: hash("recovery", username)})
		awaitControl(t, c, "SET_KEYS ACK", func(m protocol.Control) bool { return m.Type == "ACK" && m.AccountKeys != nil })
		testIDs.Store(ack.UserID, priv)
	}
	testConns.Store(c, ack.UserID)
}

// testEpoch builds a valid CREATE_EPOCH for the keyed members.
func testEpoch(userID int, a messages.Address, keys *messages.ConversationKeys) messages.NewEpoch {
	priv, _ := testIDs.Load(userID)
	identity := priv.(ed25519.PrivateKey)
	e := messages.NewEpoch{Epoch: keys.CurrentEpoch + 1, KeyCommit: filled(32)}
	for _, m := range keys.Members {
		if m.UserID == userID {
			e.CreatorKeyID = m.KeyID
		}
	}
	e.EpochSig = ed25519.Sign(identity, e2ee.EpochRecord(keys.ConversationID, testDesc(userID, a), e.Epoch, userID, e.CreatorKeyID, e.KeyCommit))
	for _, m := range keys.Members {
		e.Wraps = append(e.Wraps, testWrap(identity, keys.ConversationID, testDesc(userID, a), e.Epoch, userID, e.CreatorKeyID, m, e.KeyCommit))
	}
	return e
}

func testDesc(userID int, a messages.Address) []byte {
	if a.PeerID != 0 {
		return e2ee.ConvDesc(e2ee.KindDM, int64(min(userID, a.PeerID)), int64(max(userID, a.PeerID)))
	}
	return e2ee.ConvDesc(e2ee.KindChannel, int64(a.ServerID), int64(a.Channel))
}

func testWrap(identity ed25519.PrivateKey, conv int64, desc []byte, epoch uint32, sender int, senderKey int64, to e2ee.UserKey, commit []byte) e2ee.NewWrap {
	w := e2ee.NewWrap{RecipientID: to.UserID, RecipientKeyID: to.KeyID, Enc: filled(32), CT: filled(48)}
	signed := e2ee.WrapHead(conv, desc, epoch, sender, senderKey, to.UserID, to.KeyID, to.EncPub, commit)
	w.WrapSig = ed25519.Sign(identity, append(append(signed, w.Enc...), w.CT...))
	return w
}

// testMessage builds a signed e2ee object whose box holds text.
func testMessage(identity ed25519.PrivateKey, conv int64, sender int, keyID int64, epoch, revision uint32, msgID []byte, text string) *e2ee.Message {
	padded := 256
	for padded < len(text) {
		padded *= 2
	}
	box := make([]byte, padded+40)
	copy(box, text)
	m := &e2ee.Message{Epoch: epoch, MsgID: msgID, Revision: revision, SenderKeyID: keyID, Box: box}
	m.MsgSig = ed25519.Sign(identity, m.SignedBytes(conv, sender))
	return m
}

// textOf reads the text of a plaintext or test-encrypted message.
func textOf(content string, m *e2ee.Message) string {
	if m == nil {
		return content
	}
	return string(bytes.TrimRight(m.Box, "\x00"))
}

// seal turns a send or update request with test content into an encrypted one,
// creating the epoch directly in the store so no frames are consumed. Requests
// that the server must refuse for access get a well-formed dummy.
func seal(t *testing.T, c net.Conn, msg protocol.Control) protocol.Control {
	t.Helper()
	text := msg.DirectContent + msg.ServerContent
	msg.DirectContent, msg.ServerContent = "", ""
	msg.E2EE = &e2ee.Message{Epoch: 1, MsgID: filled(16), SenderKeyID: 1, Box: make([]byte, 296), MsgSig: make([]byte, 64)}
	id, ok := testConns.Load(c)
	if !ok {
		return msg
	}
	userID := id.(int)
	var a messages.Address
	var revision uint32
	msgID := filled(16)
	switch msg.Type {
	case "SEND_DIRECT_MESSAGE":
		a.PeerID, _ = strconv.Atoi(msg.DirectRecipientID)
	case "SEND_SERVER_MESSAGE":
		a = messages.Address{ServerID: msg.ServerID, Channel: messages.ChannelID(msg.ChannelID)}
	default:
		messageID := msg.DirectMessageID + int(msg.ServerMessageID)
		var peer, server, channel sql.NullInt64
		var old []byte
		var rev sql.NullInt64
		err := testDB.QueryRow(`
			SELECT CASE WHEN c.user_low = $2 THEN c.user_high WHEN c.user_high = $2 THEN c.user_low END,
			       sc.server_id, sc.id, m.e2ee_msg_id, m.e2ee_revision
			FROM conversation_messages m JOIN conversations c ON c.id = m.conversation_id
			LEFT JOIN server_channels sc ON sc.id = c.server_channel_id
			WHERE m.id = $1 AND m.author_id = $2`, messageID, userID).Scan(&peer, &server, &channel, &old, &rev)
		if err != nil || old == nil {
			return msg
		}
		a = messages.Address{PeerID: int(peer.Int64), ServerID: int(server.Int64), Channel: messages.ChannelID(channel.Int64)}
		msgID, revision = old, uint32(rev.Int64)+1
	}
	store := messages.NewStore(testDB)
	ctx := context.Background()
	keys, err := store.ConversationKeys(ctx, userID, a, 0)
	if err != nil {
		return msg
	}
	var keyID int64
	for _, m := range keys.Members {
		if m.UserID == userID {
			keyID = m.KeyID
		}
	}
	if keys.CurrentEpoch == 0 || keys.Stale {
		e := testEpoch(userID, a, keys)
		if _, err := store.CreateEpoch(ctx, userID, a, e); err != nil {
			t.Fatalf("create epoch: %v", err)
		}
		keys.CurrentEpoch = e.Epoch
	}
	priv, _ := testIDs.Load(userID)
	msg.E2EE = testMessage(priv.(ed25519.PrivateKey), keys.ConversationID, userID, keyID,
		keys.CurrentEpoch, revision, msgID, text)
	return msg
}
