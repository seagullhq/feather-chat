-- Restore the exact index created by 000009, so older down migrations find
-- the object they expect.
DROP INDEX IF EXISTS idx_conversation_messages_history;
CREATE INDEX idx_conversation_messages_unread
    ON conversation_messages (conversation_id, author_id, id DESC)
    WHERE deleted_at IS NULL;
