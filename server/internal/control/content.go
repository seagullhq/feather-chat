package control

import (
	"errors"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// encryptedOnly refuses plaintext content and requires an e2ee object.
func encryptedOnly(msg protocol.Control) error {
	if msg.DirectContent != "" || msg.ServerContent != "" {
		return errors.New("plaintext content refused")
	}
	if msg.E2EE == nil {
		return errors.New("encrypted message required")
	}
	return nil
}
