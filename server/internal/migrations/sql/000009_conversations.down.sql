DROP INDEX IF EXISTS idx_conversation_messages_unread;
ALTER INDEX idx_conversation_members_user_id RENAME TO idx_direct_channel_members_user_id;
ALTER INDEX idx_conversation_messages_conversation_id_id RENAME TO idx_direct_messages_channel_id_id;
CREATE INDEX IF NOT EXISTS idx_direct_channels_updated_at ON conversations (updated_at DESC);

ALTER TABLE conversation_messages
    DROP CONSTRAINT conversation_messages_reply_fkey,
    DROP CONSTRAINT conversation_messages_conv_id_key,
    ADD CONSTRAINT direct_messages_reply_to_fkey
        FOREIGN KEY (reply_to) REFERENCES conversation_messages (id) ON DELETE SET NULL;

-- Groups have no 1:1 key: drop them (cascades to members and messages).
DELETE FROM conversations WHERE kind = 'group';

ALTER TABLE conversations
    ADD COLUMN dm_key VARCHAR(255) UNIQUE;
UPDATE conversations SET dm_key = user_low || ':' || user_high;
ALTER TABLE conversations
    DROP CONSTRAINT conversations_pair_unique,
    DROP CONSTRAINT conversations_kind_pair,
    DROP CONSTRAINT conversations_pair_order,
    DROP COLUMN user_low,
    DROP COLUMN user_high,
    DROP COLUMN name,
    DROP COLUMN kind;

ALTER TABLE conversation_messages RENAME COLUMN conversation_id TO channel_id;
ALTER TABLE conversation_members RENAME COLUMN conversation_id TO channel_id;
ALTER TABLE conversation_messages RENAME TO direct_messages;
ALTER TABLE conversation_members RENAME TO direct_channel_members;
ALTER TABLE conversations RENAME TO direct_channels;
