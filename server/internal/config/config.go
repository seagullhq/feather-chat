// Package config builds the server's runtime configuration from the
// environment.
package config

import (
	"fmt"
	"net/url"
	"os"
)

// TLSDir is the directory of the control channel certificate:
// FEATHER_TLS_DIR, default ./tls.
func TLSDir() string {
	if dir := os.Getenv("FEATHER_TLS_DIR"); dir != "" {
		return dir
	}
	return "./tls"
}

// PostgresURI builds the PostgreSQL connection URI from the POSTGRES_*
// environment variables.
func PostgresURI() string {
	return fmt.Sprintf(
		"postgres://%s:%s@%s:5432/%s",
		url.QueryEscape(os.Getenv("POSTGRES_USER")),
		url.QueryEscape(os.Getenv("POSTGRES_PASSWORD")),
		os.Getenv("POSTGRES_HOST"),
		os.Getenv("POSTGRES_DB"),
	)
}
