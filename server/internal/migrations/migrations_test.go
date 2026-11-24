package migrations

import (
	"context"
	"database/sql"
	"fmt"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	_ "github.com/jackc/pgx/v5/stdlib"
	"github.com/joho/godotenv"

	"github.com/seagullhq/feather-chat/server/internal/config"
)

// TestMigrationsRoundTrip proves that 000011 and 000010 are reversible, and
// that reverting them leaves a schema the older down migrations can consume.
// It runs in a dedicated schema: the control integration tests share the same
// PostgreSQL database and must not see these DDL changes.
func TestMigrationsRoundTrip(t *testing.T) {
	_ = godotenv.Load("../../../.env")
	baseURI := config.PostgresURI()

	admin, err := sql.Open("pgx", baseURI)
	if err != nil {
		t.Skipf("postgres open: %v", err)
	}
	defer admin.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	if err := admin.PingContext(ctx); err != nil {
		t.Skipf("postgres unreachable (%s/%s): %v", envOr("POSTGRES_HOST"), envOr("POSTGRES_DB"), err)
	}

	schema := fmt.Sprintf("chat_migrate_%d", time.Now().UnixNano())
	if _, err := admin.ExecContext(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create schema: %v", err)
	}
	defer func() {
		cleanupCtx, cleanupCancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cleanupCancel()
		if _, err := admin.ExecContext(cleanupCtx, "DROP SCHEMA "+schema+" CASCADE"); err != nil {
			t.Logf("drop schema %s: %v", schema, err)
		}
	}()

	// pgx forwards search_path as a runtime parameter, so every pooled
	// connection (including golang-migrate's) uses this schema.
	u, err := url.Parse(baseURI)
	if err != nil {
		t.Fatalf("parse postgres URI: %v", err)
	}
	q := u.Query()
	q.Set("search_path", schema)
	u.RawQuery = q.Encode()
	db, err := sql.Open("pgx", u.String())
	if err != nil {
		t.Fatalf("open scoped db: %v", err)
	}
	defer db.Close()
	if err := db.PingContext(ctx); err != nil {
		t.Fatalf("ping scoped db: %v", err)
	}

	if _, err := Up(db); err != nil {
		t.Fatalf("up to latest: %v", err)
	}
	requireObject(t, db, "conversations.server_channel_id", `
		SELECT EXISTS (SELECT 1 FROM information_schema.columns
		               WHERE table_schema = current_schema()
		                 AND table_name = 'conversations' AND column_name = 'server_channel_id')`)
	requireObject(t, db, "server_chat_channels view", `SELECT to_regclass('server_chat_channels') IS NOT NULL`)
	requireObject(t, db, "readable_server_channels", `SELECT to_regproc('readable_server_channels') IS NOT NULL`)
	requireObject(t, db, "reply index", `SELECT to_regclass('idx_conversation_messages_reply') IS NOT NULL`)

	// Seed just enough rows for the FK-guarded legacy table.
	var userID, serverID, channelID int
	if err := db.QueryRowContext(ctx, `
		INSERT INTO users (username, email, password_hash)
		VALUES ('mig_owner', 'mig_owner@example.test', 'x') RETURNING id`).Scan(&userID); err != nil {
		t.Fatalf("seed user: %v", err)
	}
	if err := db.QueryRowContext(ctx, `
		INSERT INTO servers (name, owner_id, invite_code)
		VALUES ('mig server', $1, 'miginvite123') RETURNING id`, userID).Scan(&serverID); err != nil {
		t.Fatalf("seed server: %v", err)
	}
	if err := db.QueryRowContext(ctx, `
		INSERT INTO server_channels (name, channel_type, server_id)
		SELECT 'general', id, $1 FROM channel_types WHERE type_name = 'thread'
		RETURNING id`, serverID).Scan(&channelID); err != nil {
		t.Fatalf("seed channel: %v", err)
	}

	// Going to version 12 runs 000013.down: the reply index disappears.
	if err := migrateTo(db, 12); err != nil {
		t.Fatalf("down 000013: %v", err)
	}
	requireObject(t, db, "reply index removed", `SELECT to_regclass('idx_conversation_messages_reply') IS NULL`)

	// Going to version 11 runs 000012.down: the original unread index comes
	// back, proving the index-only migration is reversible too.
	if err := migrateTo(db, 11); err != nil {
		t.Fatalf("down 000012: %v", err)
	}
	requireObject(t, db, "original unread index", `SELECT to_regclass('idx_conversation_messages_unread') IS NOT NULL`)
	requireObject(t, db, "history index removed", `SELECT to_regclass('idx_conversation_messages_history') IS NULL`)

	// Going to version 10 runs 000011.down: the legacy table comes back.
	if err := migrateTo(db, 10); err != nil {
		t.Fatalf("down 000011: %v", err)
	}
	requireObject(t, db, "server_messages", `SELECT to_regclass('server_messages') IS NOT NULL`)
	if _, err := db.ExecContext(ctx, `
		INSERT INTO server_messages (channel_id, content) VALUES ($1, 'legacy')`, channelID); err != nil {
		t.Fatalf("seed legacy message: %v", err)
	}
	err = migrateTo(db, 11)
	if err == nil || !strings.Contains(err.Error(), "server_messages is not empty") {
		t.Fatalf("guarded up 000011 error = %v, want explicit non-empty guard", err)
	}
	// A failed golang-migrate up leaves version 11 marked dirty even though
	// the schema is still at 10: put the bookkeeping back before retrying.
	if _, err := db.ExecContext(ctx, `UPDATE schema_migrations SET version = 10, dirty = FALSE`); err != nil {
		t.Fatalf("restore migration version after guard: %v", err)
	}
	if _, err := db.ExecContext(ctx, `DELETE FROM server_messages`); err != nil {
		t.Fatalf("clear legacy message: %v", err)
	}
	if err := migrateTo(db, 11); err != nil {
		t.Fatalf("up 000011 after guard: %v", err)
	}
	requireObject(t, db, "server_messages dropped again", `SELECT to_regclass('server_messages') IS NULL`)

	// A server conversation with a message is exactly what the down of 000010
	// must remove before restoring the original CHECK constraints. Going to
	// version 9 runs 000011.down and then 000010.down.
	var convID int
	if err := db.QueryRowContext(ctx, `
		INSERT INTO conversations (kind, server_channel_id) VALUES ('server', $1)
		RETURNING id`, channelID).Scan(&convID); err != nil {
		t.Fatalf("seed server conversation: %v", err)
	}
	if _, err := db.ExecContext(ctx, `
		INSERT INTO conversation_messages (conversation_id, content) VALUES ($1, 'server msg')`, convID); err != nil {
		t.Fatalf("seed server message: %v", err)
	}

	if err := migrateTo(db, 9); err != nil {
		t.Fatalf("down 000010: %v", err)
	}
	requireObject(t, db, "no server conversations after down 000010", `
		SELECT NOT EXISTS (SELECT 1 FROM conversations WHERE kind = 'server')`)
	requireObject(t, db, "no orphan server messages after down 000010", `
		SELECT NOT EXISTS (SELECT 1 FROM conversation_messages WHERE conversation_id = $1)`, convID)
	requireObject(t, db, "server_channel_id removed", `
		SELECT NOT EXISTS (SELECT 1 FROM information_schema.columns
		                   WHERE table_schema = current_schema()
		                     AND table_name = 'conversations' AND column_name = 'server_channel_id')`)
	requireObject(t, db, "view removed", `SELECT to_regclass('server_chat_channels') IS NULL`)
	requireObject(t, db, "functions removed", `
		SELECT to_regproc('readable_server_channels') IS NULL
		   AND to_regproc('can_read_server_channel') IS NULL
		   AND to_regproc('reject_conversation_shape_change') IS NULL
		   AND to_regproc('cleanup_server_chat_membership') IS NULL`)

	// The restored 000009 shape must survive the earlier down migrations.
	if err := migrateTo(db, 3); err != nil {
		t.Fatalf("down through 000004: %v", err)
	}
	requireObject(t, db, "messages table restored", `SELECT to_regclass('messages') IS NOT NULL`)
	requireObject(t, db, "conversations table gone", `SELECT to_regclass('conversations') IS NULL`)

	if _, err := Up(db); err != nil {
		t.Fatalf("up again from 000003: %v", err)
	}
	requireObject(t, db, "server_channel_id restored", `
		SELECT EXISTS (SELECT 1 FROM information_schema.columns
		               WHERE table_schema = current_schema()
		                 AND table_name = 'conversations' AND column_name = 'server_channel_id')`)
	requireObject(t, db, "server_messages dropped after final up", `SELECT to_regclass('server_messages') IS NULL`)
	requireObject(t, db, "history index restored", `SELECT to_regclass('idx_conversation_messages_history') IS NOT NULL`)
	requireObject(t, db, "reply index restored", `SELECT to_regclass('idx_conversation_messages_reply') IS NOT NULL`)
}

func requireObject(t *testing.T, db *sql.DB, what, query string, args ...any) {
	t.Helper()
	var ok bool
	if err := db.QueryRow(query, args...).Scan(&ok); err != nil {
		t.Fatalf("check %s: %v", what, err)
	}
	if !ok {
		t.Fatalf("check failed: %s", what)
	}
}

func envOr(key string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return "<unset>"
}
