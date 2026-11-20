-- The old thread table is unused since the DM tables were introduced.
-- Fail loudly instead of dropping data if a deployment still uses it.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM server_messages) THEN
        RAISE EXCEPTION 'server_messages non vuota: migrare i dati prima di droppare';
    END IF;
END $$;

DROP TABLE server_messages; -- also drops idx_server_messages_channel_id_id

-- UNIQUE (conversation_id, id) already supports backward keyset pagination.
DROP INDEX IF EXISTS idx_conversation_messages_conversation_id_id;
