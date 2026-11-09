-- Server channels structures

CREATE TABLE IF NOT EXISTS channel_types (
    id BIGSERIAL PRIMARY KEY,
    type_name VARCHAR(255) NOT NULL CHECK (length(type_name) >= 1)
);

INSERT INTO channel_types (type_name) VALUES ('thread'), ('voice'), ('stage');

CREATE TABLE IF NOT EXISTS server_channels (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL CHECK (length(name) >= 1),
    channel_type BIGINT REFERENCES channel_types(id),
    server_id BIGINT REFERENCES servers(id) ON DELETE CASCADE,
    created_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Thread data structures

CREATE TABLE IF NOT EXISTS messages (
    id BIGSERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL REFERENCES server_channels(id) ON DELETE CASCADE,
    author_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
    content TEXT NOT NULL CHECK (length(content) BETWEEN 1 AND 4000),
    reply_to BIGINT REFERENCES messages(id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    edited_at TIMESTAMP WITH TIME ZONE,
    deleted_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX IF NOT EXISTS idx_messages_channel_id_id
    ON messages (channel_id, id DESC);