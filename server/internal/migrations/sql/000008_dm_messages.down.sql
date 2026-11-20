DROP INDEX IF EXISTS idx_direct_channels_updated_at;
DROP INDEX IF EXISTS idx_direct_channel_members_user_id;
DROP INDEX IF EXISTS idx_direct_messages_channel_id_id;

DROP TABLE IF EXISTS direct_channel_members;
DROP TABLE IF EXISTS direct_messages;
DROP TABLE IF EXISTS direct_channels;

ALTER INDEX IF EXISTS idx_server_messages_channel_id_id RENAME TO idx_messages_channel_id_id;
ALTER TABLE IF EXISTS server_messages RENAME TO messages;
