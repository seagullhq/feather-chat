// Package e2ee holds the server side of docs/e2ee-text.md: wire objects, signed
// byte layouts and the public checks on them. The server never holds a content
// key and never opens anything.
package e2ee

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/binary"
)

// Wire lengths in bytes.
const (
	SaltLen      = 16
	KeyLen       = 32 // auth keys, public keys, commitments, HPKE enc
	WrappedAKLen = 72
	BundleLen    = 104
	CTLen        = 48
	MsgIDLen     = 16
	boxOverhead  = 40 // nonce and tag
)

// AccountKeys is the accountKeys object. Requests carry only some fields.
type AccountKeys struct {
	KeyID       int64  `json:"keyId,omitempty"`
	IdentityPub []byte `json:"identityPub,omitempty"`
	EncPub      []byte `json:"encPub,omitempty"`
	KeySig      []byte `json:"keySig,omitempty"`
	AKPassword  []byte `json:"akPassword,omitempty"`
	AKRecovery  []byte `json:"akRecovery,omitempty"`
	Bundle      []byte `json:"bundle,omitempty"`
}

// UserKey is one public key set of a user.
type UserKey struct {
	UserID      int    `json:"userId"`
	KeyID       int64  `json:"keyId"`
	IdentityPub []byte `json:"identityPub"`
	EncPub      []byte `json:"encPub"`
	KeySig      []byte `json:"keySig"`
}

// UserKeys answers GET_USER_KEYS.
type UserKeys struct {
	UserKey
	Retired bool `json:"retired"`
}

// Message is the e2ee object of a message.
type Message struct {
	Epoch       uint32 `json:"epoch"`
	MsgID       []byte `json:"msgId"`
	Revision    uint32 `json:"revision"`
	SenderKeyID int64  `json:"senderKeyId"`
	Box         []byte `json:"box"`
	MsgSig      []byte `json:"msgSig"`
}

// NewWrap is one entry of CREATE_EPOCH wraps (no epoch) or SHARE_KEYS shares.
type NewWrap struct {
	Epoch          uint32 `json:"epoch,omitempty"`
	RecipientID    int    `json:"recipientId"`
	RecipientKeyID int64  `json:"recipientKeyId"`
	Enc            []byte `json:"enc"`
	CT             []byte `json:"ct"`
	WrapSig        []byte `json:"wrapSig"`
}

// Wrap is one of the caller's own wraps.
type Wrap struct {
	SenderID       int    `json:"senderId"`
	SenderKeyID    int64  `json:"senderKeyId"`
	RecipientKeyID int64  `json:"recipientKeyId"`
	Enc            []byte `json:"enc"`
	CT             []byte `json:"ct"`
	WrapSig        []byte `json:"wrapSig"`
}

// Epoch is one epoch as listed by GET_CONVERSATION_KEYS.
type Epoch struct {
	Epoch        uint32 `json:"epoch"`
	CreatorID    int    `json:"creatorId"`
	CreatorKeyID int64  `json:"creatorKeyId"`
	KeyCommit    []byte `json:"keyCommit"`
	EpochSig     []byte `json:"epochSig"`
	Wraps        []Wrap `json:"wraps"`
}

// Missing names a keyed member lacking a wrap.
type Missing struct {
	UserID int    `json:"userId"`
	Epoch  uint32 `json:"epoch"`
}

func label(name string) []byte { return append([]byte("FC1:"+name), 0) }

func u64(b []byte, v int64) []byte  { return binary.BigEndian.AppendUint64(b, uint64(v)) }
func u32(b []byte, v uint32) []byte { return binary.BigEndian.AppendUint32(b, v) }

// FakeSaltInput is the HMAC input of the PRELOGIN salt of an unknown
// identifier, already trimmed and lowercased.
func FakeSaltInput(identifier string) []byte { return append(label("fakesalt"), identifier...) }

// KeyRecord is what keySig signs (§3.2).
func KeyRecord(userID int, identityPub, encPub []byte) []byte {
	b := u64(label("keys"), int64(userID))
	return append(append(b, identityPub...), encPub...)
}

// Conversation kinds of convDesc.
const (
	KindDM      byte = 1 // a = lower user ID, b = higher
	KindChannel byte = 2 // a = server ID, b = channel ID
)

// ConvDesc names the address of a conversation (§4).
func ConvDesc(kind byte, a, b int64) []byte { return u64(u64([]byte{kind}, a), b) }

// EpochRecord is what epochSig signs (§4).
func EpochRecord(conv int64, desc []byte, epoch uint32, creatorID int, creatorKeyID int64, keyCommit []byte) []byte {
	b := u32(append(u64(label("epoch"), conv), desc...), epoch)
	return append(u64(u64(b, int64(creatorID)), creatorKeyID), keyCommit...)
}

// WrapHead is the head of what wrapSig signs (§4).
func WrapHead(conv int64, desc []byte, epoch uint32, senderID int, senderKeyID int64, recipientID int, recipientKeyID int64, recipientEncPub, keyCommit []byte) []byte {
	b := u32(append(u64(label("wrap"), conv), desc...), epoch)
	b = u64(u64(b, int64(senderID)), senderKeyID)
	b = u64(u64(b, int64(recipientID)), recipientKeyID)
	return append(append(b, recipientEncPub...), keyCommit...)
}

// MessageAAD binds a box to its place (§5).
func MessageAAD(conv int64, senderID int, senderKeyID int64, msgID []byte, revision, epoch uint32) []byte {
	b := u64(u64(u64(label("aad"), conv), int64(senderID)), senderKeyID)
	return u32(u32(append(b, msgID...), revision), epoch)
}

// Verify checks an Ed25519 signature, refusing wrong lengths.
func Verify(identityPub, signed, sig []byte) bool {
	return len(identityPub) == ed25519.PublicKeySize && ed25519.Verify(identityPub, signed, sig)
}

// ValidShape checks the lengths of a message: the box is 40 bytes plus a padded
// size from 256 to 16384.
func (m *Message) ValidShape() bool {
	padded := len(m.Box) - boxOverhead
	return padded >= 256 && padded <= 16384 && padded&(padded-1) == 0 &&
		len(m.MsgID) == MsgIDLen && len(m.MsgSig) == ed25519.SignatureSize && m.Epoch <= 1<<31-1
}

// SignedBytes is what msgSig signs.
func (m *Message) SignedBytes(conv int64, senderID int) []byte {
	sum := sha256.Sum256(m.Box)
	b := append(label("msg"), MessageAAD(conv, senderID, m.SenderKeyID, m.MsgID, m.Revision, m.Epoch)...)
	return append(b, sum[:]...)
}
