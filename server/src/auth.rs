//! JWT issuing + axum middleware for protected routes.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::Request,
    extract::{FromRef, FromRequestParts, State},
    http::{header::AUTHORIZATION, request::Parts},
    middleware::Next,
    response::Response,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // user id
    pub email: String,
    pub iat: i64,
    pub exp: i64,
}

pub fn hash_password(plain: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon = Argon2::default();
    let hash = argon.hash_password(plain.as_bytes(), &salt)?.to_string();
    Ok(hash)
}

pub fn verify_password(plain: &str, hash: &str) -> AppResult<bool> {
    let parsed = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

pub fn issue_token(state: &AppState, user_id: &Uuid, email: &str) -> AppResult<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::days(state.jwt_ttl_days)).timestamp(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn decode_token(secret: &str, token: &str) -> AppResult<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

/// Extractor: parses `Authorization: Bearer <jwt>` and validates it.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, s: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(s);
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("missing Authorization header".into()))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized("invalid Authorization header".into()))?;
        let claims = decode_token(&state.jwt_secret, token)?;
        let id = Uuid::parse_str(&claims.sub)
            .map_err(|_| AppError::Unauthorized("bad token subject".into()))?;
        Ok(AuthUser {
            id,
            email: claims.email,
        })
    }
}

/// Middleware alternative (used by websocket query-param auth flow).
pub async fn _require_auth(
    State(_state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    Ok(next.run(req).await)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::Mailer;
    use crate::otp::OtpStore;
    use crate::signaling::Hub;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;

    /// AppState sintético pros testes que não tocam o banco.
    /// `connect_lazy` devolve um pool sem abrir conexão — nenhum Postgres é
    /// necessário, e qualquer query acidental falharia em vez de passar batido.
    fn test_state(secret: &str, ttl_days: i64) -> AppState {
        AppState {
            db: PgPoolOptions::new()
                .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
                .expect("lazy pool"),
            hub: Arc::new(Hub::new()),
            jwt_secret: Arc::new(secret.to_string()),
            jwt_ttl_days: ttl_days,
            otp: OtpStore::new(),
            mailer: Mailer::new(None, None),
            max_peers_per_channel: 6,
        }
    }

    const SECRET: &str = "test-secret-com-pelo-menos-24-chars";

    #[test]
    fn hash_verifica_a_senha_certa_e_rejeita_a_errada() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash).unwrap());
        assert!(!verify_password("correct horse batteri", &hash).unwrap());
        assert!(!verify_password("", &hash).unwrap());
    }

    #[test]
    fn hash_e_argon2id_e_tem_salt_por_usuario() {
        let a = hash_password("mesma-senha").unwrap();
        let b = hash_password("mesma-senha").unwrap();
        assert!(a.starts_with("$argon2id$"), "esperado argon2id, veio: {a}");
        assert_ne!(a, b, "duas senhas iguais não podem gerar o mesmo hash");
    }

    #[test]
    fn verify_rejeita_hash_malformado_sem_entrar_em_panico() {
        assert!(verify_password("x", "não é um hash pha-string").is_err());
    }

    // `connect_lazy` do sqlx precisa de runtime Tokio ativo, mesmo sem conectar.
    #[tokio::test]
    async fn token_faz_round_trip_preservando_sub_e_email() {
        let state = test_state(SECRET, 30);
        let id = Uuid::new_v4();
        let token = issue_token(&state, &id, "alguem@exemplo.com").unwrap();

        let claims = decode_token(SECRET, &token).unwrap();
        assert_eq!(claims.sub, id.to_string());
        assert_eq!(claims.email, "alguem@exemplo.com");
        assert!(claims.exp > claims.iat);
    }

    // `connect_lazy` do sqlx precisa de runtime Tokio ativo, mesmo sem conectar.
    #[tokio::test]
    async fn token_assinado_com_outro_segredo_e_rejeitado() {
        let state = test_state(SECRET, 30);
        let token = issue_token(&state, &Uuid::new_v4(), "a@b.com").unwrap();

        assert!(decode_token("outro-segredo-com-24-chars-ok", &token).is_err());
    }

    // `connect_lazy` do sqlx precisa de runtime Tokio ativo, mesmo sem conectar.
    #[tokio::test]
    async fn token_expirado_e_rejeitado() {
        // TTL negativo → exp no passado. Evita sleep no teste.
        let state = test_state(SECRET, -1);
        let token = issue_token(&state, &Uuid::new_v4(), "a@b.com").unwrap();

        assert!(
            decode_token(SECRET, &token).is_err(),
            "token com exp no passado precisa ser rejeitado"
        );
    }

    // `connect_lazy` do sqlx precisa de runtime Tokio ativo, mesmo sem conectar.
    #[tokio::test]
    async fn token_adulterado_e_rejeitado() {
        let state = test_state(SECRET, 30);
        let token = issue_token(&state, &Uuid::new_v4(), "a@b.com").unwrap();

        // Troca um char do payload, mantendo o formato de 3 partes.
        let mut parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let payload = parts[1].to_string();
        let mutated = format!("{}X", &payload[..payload.len() - 1]);
        parts[1] = &mutated;

        assert!(decode_token(SECRET, &parts.join(".")).is_err());
    }

    #[test]
    fn lixo_no_lugar_do_token_nao_entra_em_panico() {
        for t in ["", "abc", "a.b.c", "Bearer x", "....."] {
            assert!(decode_token(SECRET, t).is_err(), "aceitou lixo: {t:?}");
        }
    }
}
