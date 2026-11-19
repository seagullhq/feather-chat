-- Server text channels reuse the DM conversation tables. A conversation with
-- kind='server' points at one server_channels.id; the wire keeps using that ID.

ALTER TABLE conversations DROP CONSTRAINT IF EXISTS conversations_kind_check;
ALTER TABLE conversations DROP CONSTRAINT IF EXISTS conversations_kind_pair;

ALTER TABLE conversations
    ADD COLUMN server_channel_id BIGINT UNIQUE
        REFERENCES server_channels(id) ON DELETE CASCADE;

ALTER TABLE conversations
    ADD CONSTRAINT conversations_kind_check
        CHECK (kind IN ('direct', 'group', 'server')),
    ADD CONSTRAINT conversations_kind_pair CHECK (
        (kind = 'direct' AND user_low IS NOT NULL AND user_high IS NOT NULL
                        AND server_channel_id IS NULL)
        OR (kind = 'group' AND user_low IS NULL AND user_high IS NULL
                        AND server_channel_id IS NULL)
        OR (kind = 'server' AND user_low IS NULL AND user_high IS NULL
                        AND server_channel_id IS NOT NULL)
    ),
    ADD CONSTRAINT conversations_server_name_null
        CHECK (kind <> 'server' OR name IS NULL);

-- A CHECK only sees the new row: stop a conversation changing shape after
-- creation, so the kind/server_channel_id pair stays true forever.
CREATE FUNCTION reject_conversation_shape_change() RETURNS trigger AS $$
BEGIN
    IF NEW.kind IS DISTINCT FROM OLD.kind
       OR NEW.server_channel_id IS DISTINCT FROM OLD.server_channel_id THEN
        RAISE EXCEPTION 'kind and server_channel_id are immutable';
    END IF;
    RETURN NEW;
END; $$ LANGUAGE plpgsql;

CREATE TRIGGER conversations_immutable_shape
    BEFORE UPDATE OF kind, server_channel_id ON conversations
    FOR EACH ROW EXECUTE FUNCTION reject_conversation_shape_change();

-- One gate for every server-chat read. Set-returning SQL functions with a
-- single SELECT are inlinable, so planner can push server_channel_id inside.
CREATE FUNCTION readable_server_channels(p_user BIGINT)
RETURNS TABLE (server_channel_id BIGINT, server_id BIGINT)
LANGUAGE sql STABLE AS $$
    SELECT sc.id, sc.server_id
    FROM server_channels sc
    JOIN channel_types t ON t.id = sc.channel_type AND t.type_name = 'thread'
    WHERE EXISTS (SELECT 1 FROM servers s
                  WHERE s.id = sc.server_id AND s.owner_id = p_user)
       OR EXISTS (SELECT 1 FROM server_members m
                  WHERE m.server_id = sc.server_id AND m.user_id = p_user
                    AND COALESCE(m.banned, FALSE) = FALSE);
$$;

-- Wrapper for single-channel checks; keep the table function for lists.
CREATE FUNCTION can_read_server_channel(p_user BIGINT, p_channel BIGINT) RETURNS BOOLEAN
LANGUAGE sql STABLE AS $$
    SELECT EXISTS (SELECT 1 FROM readable_server_channels(p_user) r
                   WHERE r.server_channel_id = p_channel);
$$;

CREATE VIEW server_chat_channels AS
SELECT c.id AS conversation_id, sc.id AS server_channel_id,
       sc.server_id, sc.name, c.updated_at
FROM conversations c
JOIN server_channels sc ON sc.id = c.server_channel_id
JOIN channel_types t ON t.id = sc.channel_type
WHERE c.kind = 'server' AND t.type_name = 'thread';

-- readable_server_channels filters by server first; server_members already
-- has PK (server_id, user_id) and servers has owner_id_idx.
CREATE INDEX IF NOT EXISTS idx_server_channels_server_id_type
    ON server_channels (server_id, channel_type);

-- Read pointers are a "how far" marker, not a reference to a live row.
ALTER TABLE conversation_members DROP CONSTRAINT IF EXISTS direct_channel_members_last_read_message_id_fkey;
ALTER TABLE conversation_members DROP CONSTRAINT IF EXISTS conversation_members_last_read_message_id_fkey;

-- Retention may delete a parent message without wiping the conversation_id
-- of its replies: only reply_to goes to NULL.
ALTER TABLE conversation_messages DROP CONSTRAINT IF EXISTS conversation_messages_reply_fkey;
ALTER TABLE conversation_messages ADD CONSTRAINT conversation_messages_reply_fkey
    FOREIGN KEY (conversation_id, reply_to)
    REFERENCES conversation_messages (conversation_id, id)
    ON DELETE SET NULL (reply_to);

-- Soft-deleted messages keep an empty content and free their TOAST chunks.
ALTER TABLE conversation_messages DROP CONSTRAINT IF EXISTS direct_messages_content_check;
ALTER TABLE conversation_messages DROP CONSTRAINT IF EXISTS conversation_messages_content_check;
ALTER TABLE conversation_messages ADD CONSTRAINT conversation_messages_content_check
    CHECK (deleted_at IS NOT NULL OR (length(btrim(content)) BETWEEN 1 AND 4000));

-- Membership rows are only a read pointer: drop them on leave/ban. A single
-- DELETE OR UPDATE trigger cannot use NEW or TG_OP in its WHEN clause.
CREATE FUNCTION cleanup_server_chat_membership() RETURNS trigger AS $$
BEGIN
    DELETE FROM conversation_members cm
    USING conversations c
    JOIN server_channels sc ON sc.id = c.server_channel_id
    WHERE cm.conversation_id = c.id
      AND cm.user_id = OLD.user_id
      AND sc.server_id = OLD.server_id;
    RETURN NULL;
END; $$ LANGUAGE plpgsql;

CREATE TRIGGER server_chat_membership_cleanup_del
    AFTER DELETE ON server_members
    FOR EACH ROW EXECUTE FUNCTION cleanup_server_chat_membership();

CREATE TRIGGER server_chat_membership_cleanup_ban
    AFTER UPDATE OF banned ON server_members
    FOR EACH ROW
    WHEN (COALESCE(NEW.banned, FALSE) AND NOT COALESCE(OLD.banned, FALSE))
    EXECUTE FUNCTION cleanup_server_chat_membership();
