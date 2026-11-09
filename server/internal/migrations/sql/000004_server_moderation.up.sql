-- Server moderation structures

CREATE TABLE IF NOT EXISTS permission_levels (
    id BIGSERIAL PRIMARY KEY,
    permission_id VARCHAR(255),
    permission_name VARCHAR(255)
);

INSERT INTO permission_levels (permission_id, permission_name) VALUES
    ('ADMIN', 'Admin'),
    ('HEAD_MODERATOR', 'Head Moderator'),
    ('MODERATOR', 'Moderator'),
    ('USER', 'User');

CREATE TABLE IF NOT EXISTS server_permissions (
    id BIGSERIAL PRIMARY KEY,
    perm_level BIGINT REFERENCES permission_levels(id) ON DELETE SET NULL,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    server_id BIGINT REFERENCES servers(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);
