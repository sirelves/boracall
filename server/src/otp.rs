//! Simple in-memory OTP store. (purpose, email) → (code, expires_at).
//! Lost on restart — acceptable for MVP. Migrate to DB-backed in next pass.
//!
//! Used for two purposes: verify email (6 digit / 10min) and password reset
//! (8 digit / 30min). Namespaced by purpose so one doesn't consume the other.

use dashmap::DashMap;
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};

const VERIFY_TTL: Duration = Duration::from_secs(10 * 60);
const RESET_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
pub struct OtpStore {
    inner: Arc<DashMap<String, (String, Instant)>>,
}

impl OtpStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    fn key(purpose: &str, email: &str) -> String {
        format!("{}:{}", purpose, email.to_lowercase())
    }

    fn issue_n(&self, purpose: &str, email: &str, digits: usize, ttl: Duration) -> String {
        let code: String = (0..digits)
            .map(|_| rand::thread_rng().gen_range(0..10).to_string())
            .collect();
        self.inner.insert(
            Self::key(purpose, email),
            (code.clone(), Instant::now() + ttl),
        );
        code
    }

    fn verify_n(&self, purpose: &str, email: &str, code: &str) -> bool {
        let key = Self::key(purpose, email);
        let matched = self
            .inner
            .get(&key)
            .map(|e| e.0 == code && Instant::now() < e.1)
            .unwrap_or(false);
        if matched {
            self.inner.remove(&key);
        }
        matched
    }

    /// Email verification — 6 digits, 10 min TTL.
    pub fn issue(&self, email: &str) -> String {
        self.issue_n("verify", email, 6, VERIFY_TTL)
    }
    pub fn verify(&self, email: &str, code: &str) -> bool {
        self.verify_n("verify", email, code)
    }

    /// Password reset — 8 digits, 30 min TTL.
    pub fn issue_reset(&self, email: &str) -> String {
        self.issue_n("reset", email, 8, RESET_TTL)
    }
    pub fn verify_reset(&self, email: &str, code: &str) -> bool {
        self.verify_n("reset", email, code)
    }
}
