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

        Ok(Self {
            bind,
            database_url,
            jwt_secret,
            jwt_ttl_days,
            cors_allow_any,
            log,
        })
    }
}
