-- Direct and group chats share one set of tables. A 1:1 conversation is
-- identified by its two users (user_low < user_high), so the pair is unique
-- by constraint; groups leave the pair NULL (NULLs never collide in UNIQUE).
ALTER TABLE direct_channels RENAME TO conversations;
ALTER TABLE direct_channel_members RENAME TO conversation_members;
ALTER TABLE direct_messages RENAME TO conversation_messages;
ALTER TABLE conversation_members RENAME COLUMN channel_id TO conversation_id;
ALTER TABLE conversation_messages RENAME COLUMN channel_id TO conversation_id;

ALTER TABLE conversations
    ADD COLUMN kind VARCHAR(10) NOT NULL DEFAULT 'direct' CHECK (kind IN ('direct', 'group')),
    ADD COLUMN name VARCHAR(100),
    ADD COLUMN user_low BIGINT,
    ADD COLUMN user_high BIGINT;

UPDATE conversations
SET user_low = split_part(dm_key, ':', 1)::BIGINT,
    user_high = split_part(dm_key, ':', 2)::BIGINT
WHERE dm_key IS NOT NULL;
UPDATE conversations SET kind = 'group' WHERE dm_key IS NULL;

ALTER TABLE conversations
    DROP COLUMN dm_key,
    ALTER COLUMN kind DROP DEFAULT,
    ADD CONSTRAINT conversations_pair_order CHECK (user_low < user_high),
    ADD CONSTRAINT conversations_kind_pair CHECK (
        (kind = 'direct' AND user_low IS NOT NULL AND user_high IS NOT NULL)
        OR (kind = 'group' AND user_low IS NULL AND user_high IS NULL)),
    ADD CONSTRAINT conversations_pair_unique UNIQUE (user_low, user_high);

-- A reply must point at a message of the same conversation.
ALTER TABLE conversation_messages
    DROP CONSTRAINT IF EXISTS direct_messages_reply_to_fkey,
    ADD CONSTRAINT conversation_messages_conv_id_key UNIQUE (conversation_id, id),
    ADD CONSTRAINT conversation_messages_reply_fkey
        FOREIGN KEY (conversation_id, reply_to) REFERENCES conversation_messages (conversation_id, id);

-- Nothing can use an index on updated_at alone: the sidebar filters by member first.
DROP INDEX IF EXISTS idx_direct_channels_updated_at;
ALTER INDEX idx_direct_messages_channel_id_id RENAME TO idx_conversation_messages_conversation_id_id;
ALTER INDEX idx_direct_channel_members_user_id RENAME TO idx_conversation_members_user_id;

-- Unread counts: live messages of one author after the read pointer.
CREATE INDEX idx_conversation_messages_unread
    ON conversation_messages (conversation_id, author_id, id DESC)
    WHERE deleted_at IS NULL;
