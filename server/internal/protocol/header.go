// Package protocol defines the feather-chat wire formats: the 5-byte UDP
// media header (§3), the length-prefixed JSON framing of TCP control
// messages (§7), and those messages' payload types.
package protocol

import (
	"encoding/binary"
	"errors"
)

// UDP media header (§3): FL | SSRC(u32) where FL = V(2)|T(1)|K(5).
const HeaderLen = 5

const (
	KindAudio uint8 = 0
	KindPing  uint8 = 1
)

// Header is the decoded UDP media header.
type Header struct {
	Version    uint8
	Terminator bool
	Kind       uint8
	SSRC       uint32
}

// EncodeHeader packs h into its 5 wire bytes.
func EncodeHeader(h Header) []byte {
	b := make([]byte, HeaderLen)
	var t byte
	if h.Terminator {
		t = 0x20
	}
	b[0] = h.Version<<6 | t | h.Kind&0x1f
	binary.BigEndian.PutUint32(b[1:], h.SSRC)
	return b
}

// DecodeHeader unpacks the first HeaderLen bytes of b.
func DecodeHeader(b []byte) (Header, error) {
	if len(b) < HeaderLen {
		return Header{}, errors.New("short header")
	}
	fl := b[0]
	return Header{
		Version:    fl >> 6,
		Terminator: fl&0x20 != 0,
		Kind:       fl & 0x1f,
		SSRC:       binary.BigEndian.Uint32(b[1:]),
	}, nil
}
