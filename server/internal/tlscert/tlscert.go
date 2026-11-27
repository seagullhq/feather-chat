// Package tlscert loads or creates the certificate of the TLS control channel
// (transport-security.md §1). Clients pin its fingerprint, so an existing key
// is never replaced.
package tlscert

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/hex"
	"encoding/pem"
	"errors"
	"fmt"
	"io/fs"
	"math/big"
	"os"
	"path/filepath"
	"time"
)

// Load returns the operator's certFile/keyFile pair (both or neither).
// Without them it uses cert.pem and key.pem in dir, generating the pair only
// when both files are missing.
func Load(certFile, keyFile, dir string) (tls.Certificate, error) {
	if (certFile == "") != (keyFile == "") {
		return tls.Certificate{}, errors.New("-tls-cert and -tls-key must be given together")
	}
	if certFile == "" {
		certFile, keyFile = filepath.Join(dir, "cert.pem"), filepath.Join(dir, "key.pem")
		_, certErr := os.Stat(certFile)
		_, keyErr := os.Stat(keyFile)
		if errors.Is(certErr, fs.ErrNotExist) && errors.Is(keyErr, fs.ErrNotExist) {
			if err := generate(dir, certFile, keyFile); err != nil {
				return tls.Certificate{}, fmt.Errorf("generate certificate: %w", err)
			}
		}
	}
	// A missing, unparsable or mismatched file fails here.
	return tls.LoadX509KeyPair(certFile, keyFile)
}

// Fingerprint is the lowercase hex SHA-256 of the DER leaf certificate.
func Fingerprint(cert tls.Certificate) string {
	sum := sha256.Sum256(cert.Certificate[0])
	return hex.EncodeToString(sum[:])
}

// Config is the server side of the control channel: TLS 1.3 only, no session
// resumption.
func Config(cert tls.Certificate) *tls.Config {
	return &tls.Config{
		Certificates:           []tls.Certificate{cert},
		MinVersion:             tls.VersionTLS13,
		SessionTicketsDisabled: true,
	}
}

// generate writes a self-signed ECDSA P-256 pair, valid for 20 years.
func generate(dir, certFile, keyFile string) error {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return err
	}
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 127))
	if err != nil {
		return err
	}
	now := time.Now()
	template := &x509.Certificate{
		SerialNumber:          serial,
		Subject:               pkix.Name{CommonName: "feather-chat"},
		NotBefore:             now.Add(-time.Hour), // tolerate skewed clocks
		NotAfter:              now.AddDate(20, 0, 0),
		KeyUsage:              x509.KeyUsageDigitalSignature,
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		BasicConstraintsValid: true,
	}
	certDER, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		return err
	}
	keyDER, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	if err := writePEM(keyFile, "PRIVATE KEY", keyDER, 0o600); err != nil {
		return err
	}
	return writePEM(certFile, "CERTIFICATE", certDER, 0o644)
}

// writePEM creates path, refusing to overwrite an existing file.
func writePEM(path, kind string, der []byte, mode os.FileMode) error {
	f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
	if err != nil {
		return err
	}
	return errors.Join(pem.Encode(f, &pem.Block{Type: kind, Bytes: der}), f.Close())
}
