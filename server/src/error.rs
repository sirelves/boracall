//! Unified error type with axum IntoResponse — every handler returns AppResult<T>.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("password hash: {0}")]
    Argon(argon2::password_hash::Error),
    #[error(transparent)]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
            AppError::Db(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Db(_) | AppError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
            AppError::Argon(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
            AppError::Jwt(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
        };

        // Log details at the server side; clients see a compact message only.
        if status.is_server_error() {
            tracing::error!(error = ?self, "server error");
        } else {
            tracing::debug!(error = ?self, "client error");
        }

        let message = match &self {
            AppError::Db(sqlx::Error::RowNotFound) => "not found".to_string(),
            AppError::Db(_) | AppError::Internal(_) | AppError::Argon(_) => {
                "internal error".to_string()
            }
            AppError::Jwt(_) => "unauthorized".to_string(),
            other => other.to_string(),
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

impl From<validator::ValidationErrors> for AppError {
    fn from(e: validator::ValidationErrors) -> Self {
        AppError::Validation(e.to_string())
    }
}

impl From<argon2::password_hash::Error> for AppError {
    fn from(e: argon2::password_hash::Error) -> Self {
        AppError::Argon(e)
    }
}
