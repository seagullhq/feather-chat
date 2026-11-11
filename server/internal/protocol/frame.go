package protocol

import (
	"encoding/binary"
	"io"
)

// maxControl bounds a control frame, so a hostile length prefix cannot pin
// memory: zero-length frames are rejected as malformed, 1 MiB and above too.
const maxControl = 1 << 20

// WriteFrame emits §7 control framing: u32 big-endian length + JSON payload.
func WriteFrame(w io.Writer, payload []byte) error {
	var hdr [4]byte
	binary.BigEndian.PutUint32(hdr[:], uint32(len(payload)))
	_, err := w.Write(append(hdr[:], payload...))
	return err
}

// ReadFrame reads one §7 control frame, returning its JSON payload.
func ReadFrame(r io.Reader) ([]byte, error) {
	var hdr [4]byte
	if _, err := io.ReadFull(r, hdr[:]); err != nil {
		return nil, err
	}
	n := binary.BigEndian.Uint32(hdr[:])
	if n == 0 || n >= maxControl {
		return nil, io.ErrUnexpectedEOF
	}
	payload := make([]byte, n)
	_, err := io.ReadFull(r, payload)
	return payload, err
}
