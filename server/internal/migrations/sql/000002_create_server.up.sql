-- 1. Creazione della tabella servers
CREATE TABLE IF NOT EXISTS servers (
    id BIGSERIAL PRIMARY KEY,

    -- minLength: 3, maxLength: 32
    name VARCHAR(32) NOT NULL CHECK (length(name) >= 3),

    -- Foreign key che punta alla tabella users creata nella migrazione 1
    owner_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- minLength: 6, maxLength: 32
    invite_code VARCHAR(32) NOT NULL CHECK (length(invite_code) >= 6),

    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- 2. Indici per la tabella servers
CREATE INDEX owner_id_idx ON servers (owner_id);
CREATE UNIQUE INDEX invite_code_unique_idx ON servers (invite_code);

-- 3. Tabella di Join per l'array "members"
-- Sostituisce l'array di ObjectId di MongoDB
CREATE TABLE IF NOT EXISTS server_members (
    server_id BIGINT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Aggiunta utile che in Mongo non avevi esplicita nel nested array
    joined_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),

    -- La primary key composta garantisce che un utente non possa entrare due volte nello stesso server
    PRIMARY KEY (server_id, user_id)
);

-- Indice per cercare velocemente a quali server appartiene un utente
CREATE INDEX server_members_user_idx ON server_members (user_id);
