package main

import "embed"

// migrationFS holds the SQL migration scripts. Embedding them makes the server
// independent of its working directory: a `file://` path would be resolved
// against the process CWD, so the binary would only find them when started
// from server/.
//
//go:embed internal/migrations/sql/*.sql
var migrationFS embed.FS
