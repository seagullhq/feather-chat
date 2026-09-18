package main

import (
	"bytes"
	"testing"
)

// TestHeaderRoundTrip verifies encode/decode are inverse across every flag
// combination. Each case is a subtest so `go test -v` names it.
func TestHeaderRoundTrip(t *testing.T) {
	cases := []struct {
		name string
		h    Header
	}{
		{"audio", Header{Version: 0, Terminator: false, Kind: KindAudio, SSRC: 0x11223344}},
		{"audio_terminator", Header{Version: 0, Terminator: true, Kind: KindAudio, SSRC: 0}},
		{"ping", Header{Version: 0, Terminator: false, Kind: KindPing, SSRC: 0xdeadbeef}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			enc := EncodeHeader(tc.h)
			got, err := DecodeHeader(enc)
			if err != nil {
				t.Fatal(err)
			}
			t.Logf("encoded %x -> %+v", enc, got)
			if got != tc.h {
				t.Fatalf("round trip: got %+v want %+v", got, tc.h)
			}
		})
	}
}

func TestHeaderShortRejected(t *testing.T) {
	t.Run("short_header_rejected", func(t *testing.T) {
		if _, err := DecodeHeader([]byte{0x00, 0x11, 0x22}); err == nil {
			t.Fatal("expected error on header shorter than 5 bytes")
		}
	})
}

// TestHeaderGolden pins the exact bytes from the wire-format worked examples (§8).
func TestHeaderGolden(t *testing.T) {
	t.Run("audio_ssrc_0x11223344", func(t *testing.T) {
		// §8: header: kind=0, V=0, T=0, SSRC=0x11223344
		want := []byte{0x00, 0x11, 0x22, 0x33, 0x44}
		got := EncodeHeader(Header{Kind: KindAudio, SSRC: 0x11223344})
		t.Logf("want %x got %x", want, got)
		if !bytes.Equal(got, want) {
			t.Fatalf("golden audio: got %x want %x", got, want)
		}
	})
	t.Run("ping_ssrc_0x11223344", func(t *testing.T) {
		// §8: header (kind=1) carries SSRC; payload timestamp is client-side.
		want := []byte{0x01, 0x11, 0x22, 0x33, 0x44}
		got := EncodeHeader(Header{Kind: KindPing, SSRC: 0x11223344})
		t.Logf("want %x got %x", want, got)
		if !bytes.Equal(got, want) {
			t.Fatalf("golden ping: got %x want %x", got, want)
		}
	})
}
