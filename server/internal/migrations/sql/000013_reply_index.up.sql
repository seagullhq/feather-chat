-- The composite self-FK on (conversation_id, reply_to) uses ON DELETE SET NULL.
-- PostgreSQL must find the rows that reference each deleted message; without
-- an index on the referencing columns it scans the whole conversation for
-- every row, making the ON DELETE CASCADE of a busy channel quadratic.
CREATE INDEX IF NOT EXISTS idx_conversation_messages_reply
    ON conversation_messages (conversation_id, reply_to)
    WHERE reply_to IS NOT NULL;
