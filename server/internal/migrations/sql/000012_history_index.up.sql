-- The original unread index keys on (conversation_id, author_id, id DESC),
-- so a newest-first history query cannot use it for ordering and falls back
-- to a Sort. A narrower partial index serves both history and the capped
-- unread count: author_id is a filter there, not an equality key.
DROP INDEX IF EXISTS idx_conversation_messages_unread;
CREATE INDEX idx_conversation_messages_history
    ON conversation_messages (conversation_id, id DESC)
    WHERE deleted_at IS NULL;
