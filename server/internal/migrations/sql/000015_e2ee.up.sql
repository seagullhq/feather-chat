-- End-to-end encryption for text (docs/e2ee-text.md). The server stores only
-- verifiers, public keys and ciphertext.
ALTER TABLE users
    ADD COLUMN auth_scheme SMALLINT NOT NULL DEFAULT 0 CHECK (auth_scheme IN (0, 1)),
    ADD COLUMN auth_salt BYTEA,
    ADD COLUMN auth_verifier BYTEA,
    ADD CONSTRAINT users_auth_scheme_fields CHECK
        (auth_scheme = 0 OR (auth_salt IS NOT NULL AND auth_verifier IS NOT NULL));

-- One row of server secrets, filled by the server on first use. fake_salt keys
-- the PRELOGIN salt of unknown identifiers.
CREATE TABLE server_secrets (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    fake_salt BYTEA NOT NULL
);

-- No foreign key to users: public keys outlive their account, so old
-- signatures still verify. id is the wire keyId.
CREATE TABLE user_keys (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    identity_pub BYTEA NOT NULL,
    enc_pub BYTEA NOT NULL,
    key_sig BYTEA NOT NULL,
    ak_password BYTEA,
    ak_recovery BYTEA,
    bundle BYTEA,
    recovery_verifier BYTEA,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    retired_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX user_keys_current_idx ON user_keys (user_id) WHERE retired_at IS NULL;
CREATE INDEX user_keys_user_idx ON user_keys (user_id);

-- creator_id, user_id and sender_id have no foreign key on purpose.
CREATE TABLE conversation_epochs (
    conversation_id BIGINT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    epoch INTEGER NOT NULL CHECK (epoch > 0),
    creator_id BIGINT NOT NULL,
    creator_key_id BIGINT NOT NULL,
    key_commit BYTEA NOT NULL,
    epoch_sig BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Set when a rejected wrap of the creator made the epoch stale.
    rejected_by BIGINT,
    rejected_at TIMESTAMPTZ,
    PRIMARY KEY (conversation_id, epoch)
);
CREATE INDEX conversation_epochs_rejected_idx
    ON conversation_epochs (conversation_id, rejected_by, rejected_at) WHERE rejected_by IS NOT NULL;

CREATE TABLE epoch_recipients (
    conversation_id BIGINT NOT NULL,
    epoch INTEGER NOT NULL,
    user_id BIGINT NOT NULL,
    key_id BIGINT NOT NULL,
    PRIMARY KEY (conversation_id, epoch, user_id, key_id),
    FOREIGN KEY (conversation_id, epoch) REFERENCES conversation_epochs ON DELETE CASCADE
);
CREATE INDEX epoch_recipients_user_idx ON epoch_recipients (user_id);

CREATE TABLE epoch_wraps (
    id BIGSERIAL PRIMARY KEY,
    conversation_id BIGINT NOT NULL,
    epoch INTEGER NOT NULL,
    recipient_id BIGINT NOT NULL,
    recipient_key_id BIGINT NOT NULL,
    sender_id BIGINT NOT NULL,
    sender_key_id BIGINT NOT NULL,
    enc BYTEA NOT NULL,
    ct BYTEA NOT NULL,
    wrap_sig BYTEA NOT NULL,
    rejected BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (conversation_id, epoch, recipient_id, sender_id),
    FOREIGN KEY (conversation_id, epoch) REFERENCES conversation_epochs ON DELETE CASCADE
);
CREATE INDEX epoch_wraps_recipient_idx ON epoch_wraps (recipient_id, conversation_id, epoch, id);

-- Deleting an account drops its secrets and retires its keys.
CREATE FUNCTION retire_deleted_user_keys() RETURNS trigger AS $$
BEGIN
    UPDATE user_keys SET retired_at = COALESCE(retired_at, now()),
        ak_password = NULL, ak_recovery = NULL, bundle = NULL, recovery_verifier = NULL
    WHERE user_id = OLD.id;
    DELETE FROM epoch_wraps WHERE recipient_id = OLD.id;
    RETURN NULL;
END; $$ LANGUAGE plpgsql;

CREATE TRIGGER users_retire_keys AFTER DELETE ON users
    FOR EACH ROW EXECUTE FUNCTION retire_deleted_user_keys();

-- Members of a conversation: a DM's two users, or a channel's server owner and
-- non-banned members.
CREATE FUNCTION conversation_member_ids(p_conversation BIGINT) RETURNS TABLE (user_id BIGINT)
LANGUAGE sql STABLE AS $$
    SELECT u.id FROM conversations c
    JOIN users u ON u.id IN (c.user_low, c.user_high)
    WHERE c.id = p_conversation
    UNION
    SELECT s.owner_id FROM conversations c
    JOIN server_channels sc ON sc.id = c.server_channel_id
    JOIN servers s ON s.id = sc.server_id
    WHERE c.id = p_conversation
    UNION
    SELECT m.user_id FROM conversations c
    JOIN server_channels sc ON sc.id = c.server_channel_id
    JOIN server_members m ON m.server_id = sc.server_id
    WHERE c.id = p_conversation AND COALESCE(m.banned, FALSE) = FALSE;
$$;

-- e2ee_sender repeats author_id but survives account deletion: signature and
-- key derivation cover it. Deleting a message clears box and signature and
-- keeps the id, so it cannot be replayed.
ALTER TABLE conversation_messages
    ADD COLUMN e2ee_sender BIGINT,
    ADD COLUMN e2ee_sender_key BIGINT,
    ADD COLUMN e2ee_epoch INTEGER,
    ADD COLUMN e2ee_msg_id BYTEA,
    ADD COLUMN e2ee_revision BIGINT,
    ADD COLUMN e2ee_box BYTEA,
    ADD COLUMN e2ee_sig BYTEA;
CREATE UNIQUE INDEX conversation_messages_msg_id_idx
    ON conversation_messages (conversation_id, e2ee_sender, e2ee_msg_id) WHERE e2ee_msg_id IS NOT NULL;
CREATE INDEX conversation_messages_epoch_idx
    ON conversation_messages (conversation_id, e2ee_epoch) WHERE e2ee_epoch IS NOT NULL;

ALTER TABLE conversation_messages DROP CONSTRAINT conversation_messages_content_check;
ALTER TABLE conversation_messages ADD CONSTRAINT conversation_messages_content_check CHECK (
    deleted_at IS NOT NULL
    OR (e2ee_box IS NOT NULL AND content = '')
    OR (e2ee_box IS NULL AND length(btrim(content)) BETWEEN 1 AND 4000));
