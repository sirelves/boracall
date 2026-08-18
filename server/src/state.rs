//! Shared application state — cloned into every handler and WebSocket task.

use crate::email::Mailer;
use crate::otp::OtpStore;
use crate::ratelimit::RateLimiter;
use crate::signaling::Hub;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub hub: Arc<Hub>,
    pub jwt_secret: Arc<String>,
    pub jwt_ttl_days: i64,
    pub otp: OtpStore,
    pub mailer: Mailer,
    pub stun_urls: Vec<String>,
    pub turn_urls: Vec<String>,
    pub turn_secret: Option<Arc<String>>,
    pub turn_ttl_secs: i64,
    /// Limite por IP nas rotas de autenticação.
    pub limite_ip: RateLimiter,
    /// Limite por e-mail de destino nas rotas que disparam mensagem.
    pub limite_email: RateLimiter,
    pub max_peers_per_channel: usize,
}
