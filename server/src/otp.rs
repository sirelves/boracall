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

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EMAIL: &str = "pessoa@exemplo.com";

    #[test]
    fn codigo_de_verificacao_tem_6_digitos() {
        let code = OtpStore::new().issue(EMAIL);
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()), "veio: {code}");
    }

    #[test]
    fn codigo_de_reset_tem_8_digitos() {
        let code = OtpStore::new().issue_reset(EMAIL);
        assert_eq!(code.len(), 8);
        assert!(code.chars().all(|c| c.is_ascii_digit()), "veio: {code}");
    }

    #[test]
    fn codigo_valido_passa_uma_vez_so() {
        let store = OtpStore::new();
        let code = store.issue(EMAIL);

        assert!(
            store.verify(EMAIL, &code),
            "primeira verificação deve passar"
        );
        assert!(
            !store.verify(EMAIL, &code),
            "código é de uso único — a segunda tem que falhar"
        );
    }

    #[test]
    fn codigo_errado_nao_passa_e_nao_consome_o_certo() {
        let store = OtpStore::new();
        let code = store.issue(EMAIL);
        let errado = if code == "000000" { "111111" } else { "000000" };

        assert!(!store.verify(EMAIL, errado));
        assert!(
            store.verify(EMAIL, &code),
            "tentativa errada não pode invalidar o código legítimo"
        );
    }

    #[test]
    fn email_e_case_insensitive() {
        let store = OtpStore::new();
        let code = store.issue("Pessoa@Exemplo.COM");
        assert!(store.verify("pessoa@exemplo.com", &code));
    }

    #[test]
    fn codigo_de_outro_email_nao_serve() {
        let store = OtpStore::new();
        let code = store.issue(EMAIL);
        assert!(!store.verify("outra@exemplo.com", &code));
    }

    #[test]
    fn verify_e_reset_nao_se_consomem() {
        let store = OtpStore::new();
        let verify_code = store.issue(EMAIL);
        let reset_code = store.issue_reset(EMAIL);

        // O código de verificação não vale como reset de senha, nem o contrário.
        assert!(!store.verify_reset(EMAIL, &verify_code));
        assert!(!store.verify(EMAIL, &reset_code));

        // E cada um continua válido no seu próprio namespace.
        assert!(store.verify(EMAIL, &verify_code));
        assert!(store.verify_reset(EMAIL, &reset_code));
    }

    #[test]
    fn codigo_expirado_nao_passa() {
        let store = OtpStore::new();
        // TTL zero → já nasce expirado. Evita sleep no teste.
        let code = store.issue_n("verify", EMAIL, 6, Duration::from_secs(0));
        assert!(!store.verify(EMAIL, &code));
    }

    #[test]
    fn reemitir_substitui_o_codigo_anterior() {
        let store = OtpStore::new();
        let antigo = store.issue(EMAIL);
        let novo = store.issue(EMAIL);

        assert!(!store.verify(EMAIL, &antigo), "código antigo deve morrer");
        assert!(store.verify(EMAIL, &novo));
    }
}
