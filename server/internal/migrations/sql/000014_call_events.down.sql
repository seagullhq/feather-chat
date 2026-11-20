DELETE FROM conversation_messages WHERE call IS NOT NULL;
ALTER TABLE conversation_messages
    DROP CONSTRAINT IF EXISTS conversation_messages_call_check,
    DROP COLUMN IF EXISTS call;
