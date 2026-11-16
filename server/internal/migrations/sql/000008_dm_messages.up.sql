ALTER TABLE IF EXISTS messages RENAME TO server_messages;
ALTER INDEX IF EXISTS idx_messages_channel_id_id RENAME TO idx_server_messages_channel_id_id;

CREATE TABLE IF NOT EXISTS direct_channels (
    id BIGSERIAL PRIMARY KEY,
    dm_key VARCHAR(255) UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS direct_messages (
    id BIGSERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL REFERENCES direct_channels(id) ON DELETE CASCADE,
    author_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
    content TEXT NOT NULL CHECK (length(btrim(content)) >= 1 AND length(content) <= 4000),
    reply_to BIGINT REFERENCES direct_messages(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    edited_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS direct_channel_members (
    channel_id BIGINT NOT NULL REFERENCES direct_channels(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    last_read_message_id BIGINT REFERENCES direct_messages(id) ON DELETE SET NULL,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (channel_id, user_id)
);

-- Fast keyset pagination for chat history
CREATE INDEX IF NOT EXISTS idx_direct_messages_channel_id_id
    ON direct_messages (channel_id, id DESC);

-- Fast lookup of all DMs belonging to a user
CREATE INDEX IF NOT EXISTS idx_direct_channel_members_user_id
    ON direct_channel_members (user_id);

-- Fast sorting of DM sidebar by most recent activity
CREATE INDEX IF NOT EXISTS idx_direct_channels_updated_at
    ON direct_channels (updated_at DESC);
