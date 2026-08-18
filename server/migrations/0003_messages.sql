-- BoraCall — mensagens de canal de texto.

-- ------------------------------------------------------------------
-- messages
--
-- Ordenação e paginação usam o par (created_at, id), não o id sozinho:
-- o id é UUID v4, que é aleatório e não ordena por tempo. O id entra só como
-- desempate estável entre duas mensagens do mesmo instante.
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS messages (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    edited_at   TIMESTAMPTZ
);

-- Índice casado com a query de histórico: filtra por canal e caminha pra trás
-- no par (created_at, id).
CREATE INDEX IF NOT EXISTS messages_channel_cursor_idx
    ON messages (channel_id, created_at DESC, id DESC);

-- ------------------------------------------------------------------
-- message_reads
--
-- Marcador de leitura por (canal, usuário). Guarda o timestamp além do id
-- porque a contagem de não-lidos compara por tempo — com o id de UUID v4 não
-- dá pra comparar "mais novo que".
-- ------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS message_reads (
    channel_id            UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id               UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    last_read_message_id  UUID REFERENCES messages(id) ON DELETE SET NULL,
    last_read_at          TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (channel_id, user_id)
);
