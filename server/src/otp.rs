//! Códigos de uso único: verificação de e-mail e reset de senha.
//!
//! Persistidos no Postgres. Antes viviam num `DashMap` em memória, o que dava
//! dois problemas: todo restart invalidava os códigos pendentes (a pessoa pedia
//! o código, um deploy acontecia, e ela recebia "código inválido" sem entender),
//! e dois nós nunca poderiam validar o que o outro emitiu.
//!
//! Três cuidados que a versão em memória não tinha:
//!
//! 1. **O código é guardado como hash.** Um dump do banco não entrega o código
//!    de ninguém. A janela é curta, mas o raciocínio é o mesmo da senha.
//! 2. **Tentativas erradas são contadas.** Um código de 6 dígitos cai em 10⁶
//!    chutes; sem contador, nada no servidor percebia a varredura.
//! 3. **Comparação em tempo constante**, pra não vazar quantos dígitos do
//!    prefixo estavam certos pelo tempo de resposta.
//!
//! O relógio é sempre o do banco (`NOW()`), nunca o do processo: com dois
//! relógios, alguns centésimos de segundo de diferença entre app e banco fazem
//! um código nascer "expirado" ou sobreviver além da hora.

use rand::Rng;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// Quantos chutes errados antes de queimar o código.
const MAX_TENTATIVAS: i16 = 5;

const VERIFY_TTL_MIN: i32 = 10;
const RESET_TTL_MIN: i32 = 30;

#[derive(Debug, PartialEq, Eq)]
pub enum VerifyErro {
    /// Código errado, expirado, já usado, ou nunca emitido — o chamador não
    /// distingue de propósito: dizer "esse e-mail não tem código pendente"
    /// entrega quem tem conta.
    Invalido,
    /// Estourou o número de tentativas. O código foi descartado.
    Bloqueado,
}

#[derive(Clone)]
pub struct OtpStore {
    db: PgPool,
}

fn hash(code: &str) -> String {
    let mut h = Sha256::new();
    h.update(code.as_bytes());
    format!("{:x}", h.finalize())
}

/// Compara sem retornar cedo, pra que o tempo não dependa de quantos chars
/// bateram.
fn iguais_em_tempo_constante(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut dif = 0u8;
    for i in 0..a.len() {
        dif |= a[i] ^ b[i];
    }
    dif == 0
}

fn gerar(digitos: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..digitos)
        .map(|_| rng.gen_range(0..10).to_string())
        .collect()
}

impl OtpStore {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    async fn emitir(
        &self,
        purpose: &str,
        email: &str,
        digitos: usize,
        ttl_min: i32,
    ) -> sqlx::Result<String> {
        let code = gerar(digitos);
        let email = email.trim().to_lowercase();

        // Reemitir substitui o anterior e zera o contador — quem pediu de novo
        // não deve herdar o castigo de tentativas do código velho.
        sqlx::query!(
            r#"
            INSERT INTO otp_codes (purpose, email, code_hash, expires_at, attempts)
            VALUES ($1, $2, $3, NOW() + make_interval(mins => $4::int), 0)
            ON CONFLICT (purpose, email) DO UPDATE
                SET code_hash  = EXCLUDED.code_hash,
                    expires_at = EXCLUDED.expires_at,
                    attempts   = 0,
                    created_at = NOW()
            "#,
            purpose,
            email,
            hash(&code),
            ttl_min,
        )
        .execute(&self.db)
        .await?;

        Ok(code)
    }

    async fn verificar(
        &self,
        purpose: &str,
        email: &str,
        code: &str,
    ) -> sqlx::Result<Result<(), VerifyErro>> {
        let email = email.trim().to_lowercase();

        let linha = sqlx::query!(
            r#"SELECT code_hash, attempts, (expires_at > NOW()) AS "vigente!"
               FROM otp_codes WHERE purpose = $1 AND email = $2"#,
            purpose,
            email
        )
        .fetch_optional(&self.db)
        .await?;

        let Some(linha) = linha else {
            return Ok(Err(VerifyErro::Invalido));
        };

        if !linha.vigente {
            let _ = self.descartar(purpose, &email).await;
            return Ok(Err(VerifyErro::Invalido));
        }
        if linha.attempts >= MAX_TENTATIVAS {
            let _ = self.descartar(purpose, &email).await;
            return Ok(Err(VerifyErro::Bloqueado));
        }

        if iguais_em_tempo_constante(&linha.code_hash, &hash(code)) {
            self.descartar(purpose, &email).await?; // uso único
            Ok(Ok(()))
        } else {
            sqlx::query!(
                "UPDATE otp_codes SET attempts = attempts + 1 WHERE purpose = $1 AND email = $2",
                purpose,
                email
            )
            .execute(&self.db)
            .await?;
            Ok(Err(VerifyErro::Invalido))
        }
    }

    async fn descartar(&self, purpose: &str, email: &str) -> sqlx::Result<()> {
        sqlx::query!(
            "DELETE FROM otp_codes WHERE purpose = $1 AND email = $2",
            purpose,
            email
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Remove os vencidos. Chamada periodicamente — o `expires_at` já barra o
    /// uso, isso aqui é só pra tabela não crescer sem parar.
    pub async fn limpar_vencidos(&self) -> sqlx::Result<u64> {
        let r = sqlx::query!("DELETE FROM otp_codes WHERE expires_at < NOW()")
            .execute(&self.db)
            .await?;
        Ok(r.rows_affected())
    }

    /// Verificação de e-mail — 6 dígitos, 10 minutos.
    pub async fn issue(&self, email: &str) -> sqlx::Result<String> {
        self.emitir("verify", email, 6, VERIFY_TTL_MIN).await
    }
    pub async fn verify(&self, email: &str, code: &str) -> sqlx::Result<Result<(), VerifyErro>> {
        self.verificar("verify", email, code).await
    }

    /// Reset de senha — 8 dígitos, 30 minutos.
    pub async fn issue_reset(&self, email: &str) -> sqlx::Result<String> {
        self.emitir("reset", email, 8, RESET_TTL_MIN).await
    }
    pub async fn verify_reset(
        &self,
        email: &str,
        code: &str,
    ) -> sqlx::Result<Result<(), VerifyErro>> {
        self.verificar("reset", email, code).await
    }
}

// ---------------------------------------------------------------------------
// Testes de integração — precisam de Postgres, ver handlers/servers.rs.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::servers::tests::test_state;

    fn email_unico() -> String {
        format!("otp-{}@teste.local", uuid::Uuid::new_v4())
    }

    #[test]
    fn hash_nao_devolve_o_codigo() {
        // Falha óbvia de implementação: guardar o código em claro.
        let h = hash("123456");
        assert_ne!(h, "123456");
        assert!(!h.contains("123456"));
        assert_eq!(h.len(), 64, "sha256 em hex");
        assert_eq!(h, hash("123456"), "mesmo código, mesmo hash");
        assert_ne!(h, hash("123457"));
    }

    #[test]
    fn comparacao_constante_bate_o_esperado() {
        assert!(iguais_em_tempo_constante("abc", "abc"));
        assert!(!iguais_em_tempo_constante("abc", "abd"));
        assert!(!iguais_em_tempo_constante("abc", "abcd"));
        assert!(!iguais_em_tempo_constante("", "a"));
        assert!(iguais_em_tempo_constante("", ""));
    }

    #[test]
    fn codigos_tem_o_tamanho_certo_e_sao_digitos() {
        for (n, _) in [(6, "verify"), (8, "reset")] {
            let c = gerar(n);
            assert_eq!(c.len(), n);
            assert!(c.chars().all(|c| c.is_ascii_digit()), "veio: {c}");
        }
    }

    #[tokio::test]
    #[ignore]
    async fn codigo_valido_passa_uma_vez_so() {
        let state = test_state().await;
        let email = email_unico();

        let code = state.otp.issue(&email).await.unwrap();
        assert!(state.otp.verify(&email, &code).await.unwrap().is_ok());
        assert_eq!(
            state.otp.verify(&email, &code).await.unwrap(),
            Err(VerifyErro::Invalido),
            "código é de uso único"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn o_codigo_nao_fica_em_claro_no_banco() {
        let state = test_state().await;
        let email = email_unico();
        let code = state.otp.issue(&email).await.unwrap();

        let guardado: String = sqlx::query_scalar!(
            "SELECT code_hash FROM otp_codes WHERE purpose = 'verify' AND email = $1",
            email
        )
        .fetch_one(&state.db)
        .await
        .unwrap();

        assert_ne!(guardado, code, "o código está em claro no banco");
        assert_eq!(guardado, hash(&code));
    }

    #[tokio::test]
    #[ignore]
    async fn queima_o_codigo_depois_de_cinco_erros() {
        let state = test_state().await;
        let email = email_unico();
        let code = state.otp.issue(&email).await.unwrap();
        let errado = if code.starts_with('0') {
            "999999"
        } else {
            "000000"
        };

        // Sem contador, um código de 6 dígitos cai em 10^6 chutes.
        for i in 1..=5 {
            assert_eq!(
                state.otp.verify(&email, errado).await.unwrap(),
                Err(VerifyErro::Invalido),
                "chute {i}"
            );
        }
        assert_eq!(
            state.otp.verify(&email, errado).await.unwrap(),
            Err(VerifyErro::Bloqueado),
            "o sexto chute tem que bloquear"
        );
        // E o código legítimo morre junto — quem chutou não ganha mais chances.
        assert_eq!(
            state.otp.verify(&email, &code).await.unwrap(),
            Err(VerifyErro::Invalido),
            "o código foi descartado ao bloquear"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn reemitir_zera_o_contador_de_tentativas() {
        let state = test_state().await;
        let email = email_unico();
        state.otp.issue(&email).await.unwrap();

        for _ in 0..4 {
            let _ = state.otp.verify(&email, "000000").await.unwrap();
        }

        // Pedir um código novo não pode herdar o castigo do anterior.
        let novo = state.otp.issue(&email).await.unwrap();
        assert!(
            state.otp.verify(&email, &novo).await.unwrap().is_ok(),
            "código novo devia valer, com o contador zerado"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn verify_e_reset_nao_se_consomem() {
        let state = test_state().await;
        let email = email_unico();

        let v = state.otp.issue(&email).await.unwrap();
        let r = state.otp.issue_reset(&email).await.unwrap();

        assert_eq!(
            state.otp.verify_reset(&email, &v).await.unwrap(),
            Err(VerifyErro::Invalido),
            "código de verificação não vale como reset"
        );
        assert!(state.otp.verify(&email, &v).await.unwrap().is_ok());
        assert!(state.otp.verify_reset(&email, &r).await.unwrap().is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn codigo_vencido_nao_passa_e_some_na_limpeza() {
        let state = test_state().await;
        let email = email_unico();
        state.otp.issue(&email).await.unwrap();

        // Empurra o vencimento pro passado — o relógio é o do banco.
        sqlx::query!(
            "UPDATE otp_codes SET expires_at = NOW() - interval '1 minute' WHERE email = $1",
            email
        )
        .execute(&state.db)
        .await
        .unwrap();

        assert_eq!(
            state.otp.verify(&email, "000000").await.unwrap(),
            Err(VerifyErro::Invalido)
        );
        state.otp.limpar_vencidos().await.unwrap();
        let sobrou: i64 = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "c!" FROM otp_codes WHERE email = $1"#,
            email
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(sobrou, 0);
    }

    #[tokio::test]
    #[ignore]
    async fn codigo_sobrevive_a_troca_de_instancia() {
        // O ponto todo de sair da memória: um deploy no meio do fluxo não pode
        // invalidar o código que a pessoa acabou de receber por e-mail.
        let a = test_state().await;
        let email = email_unico();
        let code = a.otp.issue(&email).await.unwrap();
        drop(a);

        let b = test_state().await; // "outro processo"
        assert!(
            b.otp.verify(&email, &code).await.unwrap().is_ok(),
            "código emitido por uma instância tem que valer na outra"
        );
    }
}
