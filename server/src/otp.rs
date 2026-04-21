//! Simple in-memory OTP store. Email → (code, expires_at).
//! Lost on restart — acceptable for MVP. Migrate to DB-backed in next pass.

use dashmap::DashMap;
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};

const OTP_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct OtpStore {
    inner: Arc<DashMap<String, (String, Instant)>>,
}

impl OtpStore {
    pub fn new() -> Self {
        Self { inner: Arc::new(DashMap::new()) }
    }

    pub fn issue(&self, email: &str) -> String {
        let code: String = (0..6)
            .map(|_| rand::thread_rng().gen_range(0..10).to_string())
            .collect();
        self.inner
            .insert(email.to_lowercase(), (code.clone(), Instant::now() + OTP_TTL));
        code
    }

    /// Returns true on match (and consumes the code).
    pub fn verify(&self, email: &str, code: &str) -> bool {
        let key = email.to_lowercase();
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
}
