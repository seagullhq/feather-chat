package protocol

import (
	"crypto/aes"
	"crypto/cipher"
	"encoding/binary"
)

// Sealed media datagrams (transport-security.md §2.2):
// FL | SSRC(u32) | counter(u64) | AES-256-GCM(payload) | tag(16),
// with the 13 header bytes as AAD.

const (
	// MediaVersion is the V field of a sealed datagram.
	MediaVersion = 1
	MediaKeyLen  = 32
	// SealedHeaderLen is the §3 header followed by the u64 counter.
	SealedHeaderLen = HeaderLen + 8
	tagLen          = 16
	// MinSealedDatagram carries one plaintext byte: empty payloads are rejected.
	MinSealedDatagram = SealedHeaderLen + 1 + tagLen
)

// NewMediaAEAD builds the AES-256-GCM instance of one media key.
func NewMediaAEAD(key []byte) (cipher.AEAD, error) {
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	return cipher.NewGCM(block)
}

// nonce is four zero bytes followed by the big-endian counter.
func nonce(counter []byte) []byte {
	n := make([]byte, 12)
	copy(n[4:], counter)
	return n
}

// SealMedia builds the datagram for payload. The version is always
// MediaVersion; the caller must never reuse a counter with the same key.
func SealMedia(aead cipher.AEAD, h Header, counter uint64, payload []byte) []byte {
	h.Version = MediaVersion
	b := make([]byte, SealedHeaderLen, SealedHeaderLen+len(payload)+tagLen)
	copy(b, EncodeHeader(h))
	binary.BigEndian.PutUint64(b[HeaderLen:], counter)
	return aead.Seal(b, nonce(b[HeaderLen:]), payload, b)
}

// OpenMedia authenticates a datagram and returns its counter and payload. The
// caller checks the header fields and the replay window.
func OpenMedia(aead cipher.AEAD, b []byte) (counter uint64, payload []byte, ok bool) {
	if len(b) < MinSealedDatagram {
		return 0, nil, false
	}
	payload, err := aead.Open(nil, nonce(b[HeaderLen:SealedHeaderLen]), b[SealedHeaderLen:], b[:SealedHeaderLen])
	if err != nil {
		return 0, nil, false
	}
	return binary.BigEndian.Uint64(b[HeaderLen:]), payload, true
}

// replayWindow is the number of counters tracked per key.
const replayWindow = 1024

// ReplayWindow rejects repeated and too old counters. The zero value is ready
// to use. It is not safe for concurrent use.
type ReplayWindow struct {
	highest uint64
	seen    [replayWindow / 64]uint64 // bit counter%replayWindow
}

// Accept records counter. ok is false for a counter already seen or older than
// highest-1023; newest reports that it is now the highest accepted. Call it
// only after the tag verified.
func (w *ReplayWindow) Accept(counter uint64) (newest, ok bool) {
	switch {
	case counter > w.highest:
		// Free the slots the window slides over.
		if counter-w.highest >= replayWindow {
			clear(w.seen[:])
		} else {
			for c := w.highest; c < counter; {
				c++
				w.seen[c%replayWindow/64] &^= 1 << (c % 64)
			}
		}
		w.highest = counter
	case w.highest-counter >= replayWindow:
		return false, false
	}
	word, bit := &w.seen[counter%replayWindow/64], uint64(1)<<(counter%64)
	if *word&bit != 0 {
		return false, false
	}
	*word |= bit
	return counter == w.highest, true
}
