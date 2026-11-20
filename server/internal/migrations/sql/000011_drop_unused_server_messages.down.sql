-- Recreate exactly what 000003 created (and 000008/000009 renamed), so the
-- older down migrations still find the objects they expect.
CREATE INDEX IF NOT EXISTS idx_conversation_messages_conversation_id_id
    ON conversation_messages (conversation_id, id DESC);

CREATE TABLE IF NOT EXISTS server_messages (
    id BIGSERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL REFERENCES server_channels(id) ON DELETE CASCADE,
    author_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
    content TEXT NOT NULL CHECK (length(content) BETWEEN 1 AND 4000),
    reply_to BIGINT REFERENCES server_messages(id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    edited_at TIMESTAMP WITH TIME ZONE,
    deleted_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX IF NOT EXISTS idx_server_messages_channel_id_id
    ON server_messages (channel_id, id DESC);
