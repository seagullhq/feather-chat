package protocol

import (
	"bytes"
	"encoding/hex"
	"testing"
)

// TestMediaTestVector pins transport-security.md §3.
func TestMediaTestVector(t *testing.T) {
	key := make([]byte, MediaKeyLen)
	for i := range key {
		key[i] = byte(i)
	}
	aead, err := NewMediaAEAD(key)
	if err != nil {
		t.Fatal(err)
	}
	got := SealMedia(aead, Header{Kind: KindAudio, SSRC: 0x11223344}, 7, []byte("feather"))
	const want = "401122334400000000000000078608726b7f722c7db028f2e43f9e807e9cce80a420a711"
	if hex.EncodeToString(got) != want {
		t.Fatalf("datagram = %x, want %s", got, want)
	}
	counter, payload, ok := OpenMedia(aead, got)
	if !ok || counter != 7 || !bytes.Equal(payload, []byte("feather")) {
		t.Fatalf("open: ok=%v counter=%d payload=%q", ok, counter, payload)
	}
	// Header (AAD), counter, ciphertext and tag are all authenticated.
	for i := range got {
		bad := bytes.Clone(got)
		bad[i] ^= 1
		if _, _, ok := OpenMedia(aead, bad); ok {
			t.Fatalf("flipped byte %d accepted", i)
		}
	}
	if _, _, ok := OpenMedia(aead, SealMedia(aead, Header{SSRC: 1}, 8, nil)); ok {
		t.Fatal("empty plaintext accepted")
	}
}

func TestReplayWindow(t *testing.T) {
	var w ReplayWindow
	for i, step := range []struct {
		counter    uint64
		newest, ok bool
	}{
		{0, true, true},
		{0, false, false},
		{5, true, true},
		{3, false, true}, // reordered, still inside the window
		{3, false, false},
		{5, false, false},
		{1028, true, true},
		{5, false, false}, // highest-1023: oldest slot, already seen
		{4, false, false}, // one older than the window
		{6, false, true},
		{1029, true, true},          // reuses the slot of counter 5
		{5000, true, true},          // jump past the whole window
		{5000 - 1023, false, true},  // oldest accepted
		{5000 - 1024, false, false}, // too old
		{5000, false, false},
		{^uint64(0), true, true}, // no overflow at the top
		{^uint64(0), false, false},
		{^uint64(0) - 1023, false, true},
	} {
		newest, ok := w.Accept(step.counter)
		if newest != step.newest || ok != step.ok {
			t.Fatalf("step %d Accept(%d) = %v, %v; want %v, %v", i, step.counter, newest, ok, step.newest, step.ok)
		}
	}
}
