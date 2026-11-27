package tlscert

import (
	"bytes"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/x509"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"testing"
	"time"
)

func TestGenerateAndReload(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "tls")
	first, err := Load("", "", dir)
	if err != nil {
		t.Fatal(err)
	}
	leaf, err := x509.ParseCertificate(first.Certificate[0])
	if err != nil {
		t.Fatal(err)
	}
	if pub, ok := leaf.PublicKey.(*ecdsa.PublicKey); !ok || pub.Curve != elliptic.P256() {
		t.Fatalf("public key = %T", leaf.PublicKey)
	}
	if leaf.NotAfter.Before(time.Now().AddDate(19, 11, 0)) {
		t.Fatalf("expires %v", leaf.NotAfter)
	}
	if runtime.GOOS != "windows" { // Windows has no POSIX mode bits
		info, err := os.Stat(filepath.Join(dir, "key.pem"))
		if err != nil || info.Mode().Perm() != 0o600 {
			t.Fatalf("key stat = %v, %v", info, err)
		}
	}
	if !regexp.MustCompile(`^[0-9a-f]{64}$`).MatchString(Fingerprint(first)) {
		t.Fatalf("fingerprint = %q", Fingerprint(first))
	}

	second, err := Load("", "", dir)
	if err != nil {
		t.Fatal(err)
	}
	if Fingerprint(second) != Fingerprint(first) {
		t.Fatal("reload changed the fingerprint")
	}
	// The operator flags load the same pair.
	flags, err := Load(filepath.Join(dir, "cert.pem"), filepath.Join(dir, "key.pem"), "unused")
	if err != nil || Fingerprint(flags) != Fingerprint(first) {
		t.Fatalf("load by flags: %v", err)
	}
}

func TestLoadNeverReplaces(t *testing.T) {
	for _, name := range []string{"cert.pem", "key.pem"} {
		for _, damage := range []string{"missing", "garbage"} {
			t.Run(damage+"_"+name, func(t *testing.T) {
				dir := t.TempDir()
				if _, err := Load("", "", dir); err != nil {
					t.Fatal(err)
				}
				path := filepath.Join(dir, name)
				var err error
				if damage == "missing" {
					err = os.Remove(path)
				} else {
					err = os.WriteFile(path, []byte("junk"), 0o600)
				}
				if err != nil {
					t.Fatal(err)
				}
				before := snapshot(dir)
				if _, err := Load("", "", dir); err == nil {
					t.Fatal("damaged pair accepted")
				}
				if !bytes.Equal(before, snapshot(dir)) {
					t.Fatal("files were rewritten")
				}
			})
		}
	}
}

func snapshot(dir string) []byte {
	cert, _ := os.ReadFile(filepath.Join(dir, "cert.pem"))
	key, _ := os.ReadFile(filepath.Join(dir, "key.pem"))
	return bytes.Join([][]byte{cert, key}, []byte("|"))
}

func TestLoadFlags(t *testing.T) {
	dir := t.TempDir()
	cert, key := filepath.Join(dir, "cert.pem"), filepath.Join(dir, "key.pem")
	for _, tc := range [][2]string{{cert, ""}, {"", key}, {cert, key}} {
		if _, err := Load(tc[0], tc[1], dir); err == nil {
			t.Fatalf("Load(%q, %q) succeeded", tc[0], tc[1])
		}
	}
	if entries, _ := os.ReadDir(dir); len(entries) != 0 {
		t.Fatal("flags must never generate files")
	}
}
