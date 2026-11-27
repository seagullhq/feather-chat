package control

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"encoding/base64"
	"encoding/binary"
	"time"
)

// A device token proves that its holder has logged in to an account before
// (transport-security.md §1.4). It never replaces the password: LOGIN with a
// valid one is limited by the token's own budget instead of the account's, so
// strangers who use up the account's budget do not lock the owner out.
const (
	deviceTokenLife = 90 * 24 * time.Hour
	nonceLen        = 16
	signedLen       = nonceLen + 8 // nonce, expiry
	tokenLen        = signedLen + sha256.Size
	deviceLabel     = "feather device token v1"
)

// deviceKey derives the signing key from the TLS private key, so tokens survive
// a restart without another secret to store. Without a key that can be encoded
// it is random.
func deviceKey(config *tls.Config) []byte {
	if config != nil && len(config.Certificates) > 0 {
		if der, err := x509.MarshalPKCS8PrivateKey(config.Certificates[0].PrivateKey); err == nil {
			mac := hmac.New(sha256.New, der)
			mac.Write([]byte(deviceLabel))
			return mac.Sum(nil)
		}
	}
	key := make([]byte, sha256.Size)
	_, _ = rand.Read(key) // never fails
	return key
}

// deviceMAC covers the password hash too: a password change voids every token.
func (s *Server) deviceMAC(userID int, passwordHash string, signed []byte) []byte {
	mac := hmac.New(sha256.New, s.deviceKey)
	mac.Write([]byte(deviceLabel))
	mac.Write(binary.BigEndian.AppendUint64(nil, uint64(userID)))
	mac.Write(signed) // fixed length, so the hash after it is unambiguous
	mac.Write([]byte(passwordHash))
	return mac.Sum(nil)
}

func (s *Server) issueDeviceToken(userID int, passwordHash string, now time.Time) string {
	b := make([]byte, nonceLen, tokenLen)
	_, _ = rand.Read(b) // never fails
	b = binary.BigEndian.AppendUint64(b, uint64(now.Add(deviceTokenLife).Unix()))
	b = append(b, s.deviceMAC(userID, passwordHash, b)...)
	return base64.RawURLEncoding.EncodeToString(b)
}

// checkDeviceToken returns the token's identity if it was issued for this
// account and password and has not expired.
func (s *Server) checkDeviceToken(token string, userID int, passwordHash string, now time.Time) (string, bool) {
	if len(token) != base64.RawURLEncoding.EncodedLen(tokenLen) {
		return "", false
	}
	b, err := base64.RawURLEncoding.Strict().DecodeString(token)
	if err != nil || !hmac.Equal(b[signedLen:], s.deviceMAC(userID, passwordHash, b[:signedLen])) {
		return "", false
	}
	if now.Unix() >= int64(binary.BigEndian.Uint64(b[nonceLen:signedLen])) {
		return "", false
	}
	return string(b[:nonceLen]), true
}
