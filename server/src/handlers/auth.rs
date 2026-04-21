//! Auth endpoints: signup, login, fake-OTP verify, me, update profile.
//!
//! Password hashing is argon2id. Tokens are stateless JWT (HS256). The OTP
//! verification route is a dev stub — it always returns success and marks
//! the user verified — because real email delivery belongs to a separate
//! milestone (Postmark/Resend).

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::auth::{hash_password, issue_token, verify_password, AuthUser};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Requests / responses
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate)]
pub struct SignupReq {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8, max = 128))]
    pub password: String,
    #[validate(length(max = 64))]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginReq {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 1))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct VerifyOtpReq {
    #[validate(length(min = 4))]
    pub code: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateMeReq {
    #[validate(length(min = 1, max = 64))]
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthResp {
    pub token: String,
    pub user: MeResp,
}

#[derive(Debug, Serialize)]
pub struct MeResp {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub email_verified: bool,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn signup(
    State(state): State<AppState>,
    Json(body): Json<SignupReq>,
) -> AppResult<Json<AuthResp>> {
    body.validate()?;

    let hash = hash_password(&body.password)?;
    let email = body.email.trim().to_lowercase();

    let row = sqlx::query!(
        r#"
        INSERT INTO users (email, password_hash, display_name)
        VALUES ($1, $2, $3)
        RETURNING id, email::text as "email!", display_name, email_verified
        "#,
        email,
        hash,
        body.display_name,
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            AppError::Conflict("email already in use".into())
        }
        _ => AppError::from(e),
    })?;

    let token = issue_token(&state, &row.id, &row.email)?;
    Ok(Json(AuthResp {
        token,
        user: MeResp {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            email_verified: row.email_verified,
        },
    }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginReq>,
) -> AppResult<Json<AuthResp>> {
    body.validate()?;
    let email = body.email.trim().to_lowercase();

    let row = sqlx::query!(
        r#"
        SELECT id, email::text as "email!", password_hash, display_name, email_verified
        FROM users WHERE email = $1
        "#,
        email
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("invalid credentials".into()))?;

    if !verify_password(&body.password, &row.password_hash)? {
        return Err(AppError::Unauthorized("invalid credentials".into()));
    }

    let token = issue_token(&state, &row.id, &row.email)?;
    Ok(Json(AuthResp {
        token,
        user: MeResp {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            email_verified: row.email_verified,
        },
    }))
}

/// Generates a fresh 6-digit code, stashes it in the OTP store, and emails it.
/// Safe to call repeatedly — re-issues the code and resets the expiry window.
pub async fn request_otp(
    auth: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<serde_json::Value>> {
    let code = state.otp.issue(&auth.email);
    state
        .mailer
        .send_otp(&auth.email, &code)
        .await
        .map_err(|e| AppError::Internal(format!("email send: {e}")))?;
    Ok(Json(serde_json::json!({ "sent": true, "to": auth.email })))
}

/// Verifies a code previously issued by /auth/request-otp and marks the user as
/// email_verified. Codes are single-use and expire after 10 minutes.
pub async fn verify_otp(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<VerifyOtpReq>,
) -> AppResult<Json<MeResp>> {
    body.validate()?;

    if !state.otp.verify(&auth.email, &body.code) {
        return Err(AppError::Unauthorized("código inválido ou expirado".into()));
    }

    let row = sqlx::query!(
        r#"
        UPDATE users SET email_verified = TRUE WHERE id = $1
        RETURNING id, email::text as "email!", display_name, email_verified
        "#,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(MeResp {
        id: row.id,
        email: row.email,
        display_name: row.display_name,
        email_verified: row.email_verified,
    }))
}

pub async fn me(auth: AuthUser, State(state): State<AppState>) -> AppResult<Json<MeResp>> {
    let row = sqlx::query!(
        r#"
        SELECT id, email::text as "email!", display_name, email_verified
        FROM users WHERE id = $1
        "#,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(MeResp {
        id: row.id,
        email: row.email,
        display_name: row.display_name,
        email_verified: row.email_verified,
    }))
}

pub async fn update_me(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateMeReq>,
) -> AppResult<Json<MeResp>> {
    body.validate()?;
    let row = sqlx::query!(
        r#"
        UPDATE users
        SET display_name = COALESCE($2, display_name)
        WHERE id = $1
        RETURNING id, email::text as "email!", display_name, email_verified
        "#,
        auth.id,
        body.display_name
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(MeResp {
        id: row.id,
        email: row.email,
        display_name: row.display_name,
        email_verified: row.email_verified,
    }))
}
