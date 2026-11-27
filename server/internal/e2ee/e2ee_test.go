package e2ee

import (
	"bytes"
	"encoding/hex"
	"testing"
)

// Test vectors of docs/e2ee-text.md §6.
func TestVectors(t *testing.T) {
	fill := func(b byte, n int) []byte { return bytes.Repeat([]byte{b}, n) }
	idPub, encPub, commit, msgID := fill(0x11, 32), fill(0x22, 32), fill(0x33, 32), fill(0x44, 16)
	for name, v := range map[string]struct {
		got  []byte
		want string
	}{
		"keyRecord": {KeyRecord(1, idPub, encPub), "4643313a6b65797300000000000000000111111111111111111111111111111111111111111111111111111111111111112222222222222222222222222222222222222222222222222222222222222222"},
		"epochRec":  {EpochRecord(3, ConvDesc(KindChannel, 8, 9), 4, 1, 5, commit), "4643313a65706f6368000000000000000003020000000000000008000000000000000900000004000000000000000100000000000000053333333333333333333333333333333333333333333333333333333333333333"},
		"wrapHead":  {WrapHead(3, ConvDesc(KindChannel, 8, 9), 4, 1, 5, 2, 6, encPub, commit), "4643313a77726170000000000000000003020000000000000008000000000000000900000004000000000000000100000000000000050000000000000002000000000000000622222222222222222222222222222222222222222222222222222222222222223333333333333333333333333333333333333333333333333333333333333333"},
		"msgAad":    {MessageAAD(3, 1, 5, msgID, 7, 4), "4643313a61616400000000000000000300000000000000010000000000000005444444444444444444444444444444440000000700000004"},
	} {
		if got := hex.EncodeToString(v.got); got != v.want {
			t.Errorf("%s = %s", name, got)
		}
	}
}

func TestMessageShape(t *testing.T) {
	m := Message{MsgID: make([]byte, 16), MsgSig: make([]byte, 64)}
	for n, ok := range map[int]bool{0: false, 40: false, 168: false, 295: false, 296: true, 297: false,
		424: false, 552: true, 16424: true, 32808: false} {
		m.Box = make([]byte, n)
		if m.ValidShape() != ok {
			t.Errorf("box of %d bytes: valid = %v", n, !ok)
		}
	}
}
