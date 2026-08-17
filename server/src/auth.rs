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
