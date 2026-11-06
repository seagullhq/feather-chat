package migrations

import "embed"

// FS holds the SQL migration scripts under "sql". Embedding them makes the
// server independent of its working directory.
//
//go:embed sql/*.sql
var FS embed.FS
