-- Ciphertext has no plaintext form: encrypted messages are lost, and accounts
-- on auth scheme 1 need a password reset.
ALTER TABLE conversation_messages DROP CONSTRAINT conversation_messages_content_check;
DELETE FROM conversation_messages WHERE e2ee_msg_id IS NOT NULL;
ALTER TABLE conversation_messages
    DROP COLUMN e2ee_sender,
    DROP COLUMN e2ee_sender_key,
    DROP COLUMN e2ee_epoch,
    DROP COLUMN e2ee_msg_id,
    DROP COLUMN e2ee_revision,
    DROP COLUMN e2ee_box,
    DROP COLUMN e2ee_sig;
ALTER TABLE conversation_messages ADD CONSTRAINT conversation_messages_content_check
    CHECK (deleted_at IS NOT NULL OR (length(btrim(content)) BETWEEN 1 AND 4000));
DROP FUNCTION conversation_member_ids(BIGINT);
DROP TRIGGER users_retire_keys ON users;
DROP FUNCTION retire_deleted_user_keys();
DROP TABLE epoch_wraps, epoch_recipients, conversation_epochs, user_keys, server_secrets;
ALTER TABLE users
    DROP CONSTRAINT users_auth_scheme_fields,
    DROP COLUMN auth_scheme,
    DROP COLUMN auth_salt,
    DROP COLUMN auth_verifier;
