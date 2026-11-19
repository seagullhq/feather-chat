-- Remove the server-chat extension and restore the exact 000009 shape.
DROP VIEW IF EXISTS server_chat_channels;
DROP FUNCTION IF EXISTS can_read_server_channel(BIGINT, BIGINT);
DROP FUNCTION IF EXISTS readable_server_channels(BIGINT);
DROP TRIGGER IF EXISTS conversations_immutable_shape ON conversations;
DROP TRIGGER IF EXISTS server_chat_membership_cleanup_del ON server_members;
DROP TRIGGER IF EXISTS server_chat_membership_cleanup_ban ON server_members;
DROP FUNCTION IF EXISTS reject_conversation_shape_change();
DROP FUNCTION IF EXISTS cleanup_server_chat_membership();
DROP INDEX IF EXISTS idx_server_channels_server_id_type;

-- Drop the three new CHECKs first: the original two reject kind='server'.
ALTER TABLE conversations DROP CONSTRAINT IF EXISTS conversations_server_name_null;
ALTER TABLE conversations DROP CONSTRAINT IF EXISTS conversations_kind_pair;
ALTER TABLE conversations DROP CONSTRAINT IF EXISTS conversations_kind_check;

-- CHECK constraints validate existing rows on ADD, so server rows must go
-- before the original constraints come back. This cascades to their messages.
DELETE FROM conversations WHERE kind = 'server';

ALTER TABLE conversations ADD CONSTRAINT conversations_kind_check
    CHECK (kind IN ('direct', 'group'));
ALTER TABLE conversations ADD CONSTRAINT conversations_kind_pair CHECK (
    (kind = 'direct' AND user_low IS NOT NULL AND user_high IS NOT NULL)
    OR (kind = 'group' AND user_low IS NULL AND user_high IS NULL));

ALTER TABLE conversation_messages DROP CONSTRAINT IF EXISTS conversation_messages_content_check;
ALTER TABLE conversation_messages ADD CONSTRAINT direct_messages_content_check
    CHECK (length(btrim(content)) >= 1 AND length(content) <= 4000);

ALTER TABLE conversation_messages DROP CONSTRAINT IF EXISTS conversation_messages_reply_fkey;
ALTER TABLE conversation_messages ADD CONSTRAINT conversation_messages_reply_fkey
    FOREIGN KEY (conversation_id, reply_to)
    REFERENCES conversation_messages (conversation_id, id);

ALTER TABLE conversation_members ADD CONSTRAINT direct_channel_members_last_read_message_id_fkey
    FOREIGN KEY (last_read_message_id) REFERENCES conversation_messages (id)
    ON DELETE SET NULL;

ALTER TABLE conversations DROP COLUMN server_channel_id;
