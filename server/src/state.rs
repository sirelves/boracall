//! Shared application state — cloned into every handler and WebSocket task.

use crate::email::Mailer;
use crate::otp::OtpStore;
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
}
