//! Runtime configuration, pulled from env with sensible defaults for dev (Colima / local docker-compose).

use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_ttl_days: i64,
    pub cors_allow_any: bool,
    pub log: String,
    pub resend_api_key: Option<String>,
    pub email_from: Option<String>,
    /// STUN e TURN entregues ao cliente em GET /api/ice.
    pub stun_urls: Vec<String>,
    pub turn_urls: Vec<String>,
    /// Sem segredo, o endpoint devolve só STUN (modo de desenvolvimento).
    pub turn_secret: Option<String>,
    pub turn_ttl_secs: i64,
    /// Limite por IP nas rotas de auth: `burst` requisições por `janela`.
    pub rl_auth_burst: u32,
    pub rl_auth_janela_secs: u64,
    /// Limite por e-mail de destino nas rotas que disparam mensagem.
    pub rl_email_burst: u32,
    pub rl_email_janela_secs: u64,
    /// Teto de pares simultâneos num único canal de voz.
    /// Acima de ~4 a mesh satura o uplink; é o guarda-corpo até o SFU existir.
    pub max_peers_per_channel: usize,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();

        let bind = std::env::var("BC_BIND")
            .unwrap_or_else(|_| "127.0.0.1:3030".to_string())
            .parse()?;

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://boracall:boracall@127.0.0.1:5432/boracall".to_string());

        let jwt_secret = std::env::var("BC_JWT_SECRET")
            .unwrap_or_else(|_| "dev-only-insecure-secret-change-in-prod-please".to_string());
        if jwt_secret.len() < 24 {
            anyhow::bail!("BC_JWT_SECRET must be at least 24 characters");
        }

        let jwt_ttl_days: i64 = std::env::var("BC_JWT_TTL_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let cors_allow_any = std::env::var("BC_CORS_ANY")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true);

        let log = std::env::var("RUST_LOG")
            .unwrap_or_else(|_| "boracall_server=info,tower_http=info,sqlx=warn".to_string());

        let resend_api_key = std::env::var("BC_RESEND_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        let email_from = std::env::var("BC_EMAIL_FROM")
            .ok()
            .filter(|s| !s.is_empty());

        // BC_MAX_PEERS_PER_ROOM segue aceito: é o nome que está nos systemd
        // units em produção hoje, e trocar o env junto com o deploy seria um
        // jeito silencioso de voltar pro default.
        // Lista separada por vírgula. O default de STUN é o do Google, que
        // resolve NAT cone; TURN só existe se for configurado.
        let lista = |chave: &str, padrao: &str| -> Vec<String> {
            std::env::var(chave)
                .unwrap_or_else(|_| padrao.to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        let stun_urls = lista(
            "BC_STUN_URLS",
            "stun:stun.l.google.com:19302,stun:stun1.l.google.com:19302",
        );
        let turn_urls = lista("BC_TURN_URLS", "");
        let turn_secret = std::env::var("BC_TURN_SECRET")
            .ok()
            .filter(|s| !s.is_empty());
        let turn_ttl_secs: i64 = std::env::var("BC_TURN_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|n: &i64| *n >= 60)
            .unwrap_or(3600);

        if turn_secret.is_some() != !turn_urls.is_empty() {
            // Um sem o outro é sempre engano de configuração, e o sintoma
            // apareceria só como chamada que não conecta pra alguns usuários.
            tracing::warn!(
                tem_segredo = turn_secret.is_some(),
                tem_urls = !turn_urls.is_empty(),
                "BC_TURN_SECRET e BC_TURN_URLS precisam ser definidos juntos — TURN desligado"
            );
        }

        let num = |chave: &str, padrao: u64| -> u64 {
            std::env::var(chave)
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|n: &u64| *n > 0)
                .unwrap_or(padrao)
        };
        // 10 tentativas por minuto por IP dá folga pra quem erra a senha e
        // aperta apertado, e ainda assim torna força bruta inviável.
        let rl_auth_burst = num("BC_RL_AUTH_BURST", 10) as u32;
        let rl_auth_janela_secs = num("BC_RL_AUTH_JANELA_SECS", 60);
        // 3 e-mails por hora pro mesmo endereço. Reenviar código duas ou três
        // vezes é normal; a quarta em uma hora é abuso.
        let rl_email_burst = num("BC_RL_EMAIL_BURST", 3) as u32;
        let rl_email_janela_secs = num("BC_RL_EMAIL_JANELA_SECS", 3600);

        let max_peers_per_channel: usize = std::env::var("BC_MAX_PEERS_PER_CHANNEL")
            .or_else(|_| std::env::var("BC_MAX_PEERS_PER_ROOM"))
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|n: &usize| *n >= 2)
            .unwrap_or(6);

        Ok(Self {
            bind,
            database_url,
            jwt_secret,
            jwt_ttl_days,
            cors_allow_any,
            log,
            resend_api_key,
            email_from,
            stun_urls,
            turn_urls,
            turn_secret,
            turn_ttl_secs,
            rl_auth_burst,
            rl_auth_janela_secs,
            rl_email_burst,
            rl_email_janela_secs,
            max_peers_per_channel,
        })
    }
}
