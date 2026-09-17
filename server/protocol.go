package main

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

type Header struct {
	Version    uint8
	Terminator bool
	Kind       uint8
	SSRC       uint32
}

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
