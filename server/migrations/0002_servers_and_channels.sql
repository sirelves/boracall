-- BoraCall — servidores com canais de texto e voz.
--
-- Substitui o modelo de "sala avulsa" por servidor → canais. As tabelas rooms e
-- memberships continuam existindo nesta migration porque o WebSocket e o front
-- antigos ainda dependem delas; saem numa migration posterior, quando nada mais
-- apontar pra lá.

-- ------------------------------------------------------------------
-- servers
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS servers (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    owner_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS servers_owner_idx ON servers (owner_id);

-- ------------------------------------------------------------------
-- server_members
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS server_members (
    server_id   UUID NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'member'
        CHECK (role IN ('owner','member')),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (server_id, user_id)
);

CREATE INDEX IF NOT EXISTS server_members_user_idx ON server_members (user_id);

-- ------------------------------------------------------------------
-- channels
--
-- `slug` é global (não por servidor) porque é ele que vira o link
-- compartilhável de canal de voz: boracall.com/c/<slug> precisa resolver
-- sozinho, sem o slug do servidor junto.
--
-- `position` ordena os canais na sidebar. Float em vez de inteiro pra permitir
-- reordenar inserindo no meio (nova = média dos vizinhos) sem reescrever a
-- coluna inteira a cada arrastar.
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS channels (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id   UUID NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL
        CHECK (kind IN ('text','voice')),
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS channels_server_idx ON channels (server_id, kind, position);

-- Dois canais do mesmo servidor não podem ter o mesmo nome dentro do mesmo tipo
-- (# geral de texto e 🔊 geral de voz convivem; dois # geral, não).
CREATE UNIQUE INDEX IF NOT EXISTS channels_server_kind_name_idx
    ON channels (server_id, kind, lower(name));
