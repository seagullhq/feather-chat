package room

import (
	"crypto/cipher"
	"crypto/rand"
	"sync/atomic"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// mediaKeys is the per-hop media crypto of one session
// (transport-security.md §2). It dies with the session.
type mediaKeys struct {
	up     cipher.AEAD           // opens what the client sends
	down   cipher.AEAD           // seals what the server sends to it
	replay protocol.ReplayWindow // of up; owned by the UDP goroutine
	path   pathState             // pending address challenge; owned by the UDP goroutine
	sent   atomic.Uint64         // next counter of down
}

// newMediaKeys draws fresh keys and returns their raw bytes for the client.
func newMediaKeys() (k *mediaKeys, up, down []byte) {
	up, down = randomKey(), randomKey()
	return &mediaKeys{up: mustAEAD(up), down: mustAEAD(down)}, up, down
}

func randomKey() []byte {
	key := make([]byte, protocol.MediaKeyLen)
	rand.Read(key) // never fails: crypto/rand crashes the process instead
	return key
}

func mustAEAD(key []byte) cipher.AEAD {
	aead, err := protocol.NewMediaAEAD(key)
	if err != nil {
		panic(err) // unreachable: the key length is fixed
	}
	return aead
}

// seal encrypts payload for this session under the next counter.
func (k *mediaKeys) seal(h protocol.Header, payload []byte) []byte {
	return protocol.SealMedia(k.down, h, k.sent.Add(1)-1, payload)
}
