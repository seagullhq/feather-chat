-- Drop only the index added by 000013; the FK and the other indexes stay.
DROP INDEX IF EXISTS idx_conversation_messages_reply;
