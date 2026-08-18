-- BoraCall — códigos de OTP e de reset de senha no banco.
--
-- Antes viviam num DashMap em memória: todo restart do servidor invalidava os
-- códigos pendentes (o usuário pedia o código, o deploy acontecia, e ele recebia
-- "código inválido" sem entender), e dois nós nunca poderiam validar o que o
-- outro emitiu.
--
-- O código é guardado como HASH, não em claro. Um dump do banco ou um SELECT
-- indevido não deve entregar o código de ninguém — vale o mesmo raciocínio da
-- senha, ainda que a janela de validade seja curta.

CREATE TABLE IF NOT EXISTS otp_codes (
    purpose     TEXT NOT NULL
        CHECK (purpose IN ('verify','reset')),
    email       CITEXT NOT NULL,
    code_hash   TEXT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    -- Tentativas erradas contra este código. Sem isso, um código de 6 dígitos
    -- cai em 10^6 chutes — e nada no servidor percebia.
    attempts    SMALLINT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (purpose, email)
);

-- Serve à limpeza dos vencidos.
CREATE INDEX IF NOT EXISTS otp_codes_expires_idx ON otp_codes (expires_at);
