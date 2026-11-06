CREATE TABLE IF NOT EXISTS users (
    -- MongoDB crea in automatico un _id, qui usiamo un BIGSERIAL (o UUID se preferisci)
    id BIGSERIAL PRIMARY KEY,

    -- minLength: 3, maxLength: 32
    username VARCHAR(32) NOT NULL CHECK (length(username) >= 3),

    email VARCHAR(255) NOT NULL,
    password_hash TEXT NOT NULL,

    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Indici univoci case-insensitive (equivalente alla collation {Locale: "en", Strength: 2})
CREATE UNIQUE INDEX username_unique_idx ON users (LOWER(username));
CREATE UNIQUE INDEX email_unique_idx ON users (LOWER(email));
