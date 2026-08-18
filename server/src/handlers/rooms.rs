//! Rooms CRUD + resolve-by-slug + join metadata.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::auth::{hash_password, verify_password, AuthUser};
use crate::error::{AppError, AppResult};
use crate::slug::random_slug;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct RoomSummary {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub room_type: String,
    pub locked: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub live: u32,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateRoomReq {
    #[validate(length(min = 2, max = 64))]
    pub name: String,
    #[validate(custom(function = "validate_room_type"))]
    pub room_type: String, // 'ephemeral' | 'persistent'
    #[serde(default)]
    pub password: Option<String>,
}

fn validate_room_type(v: &str) -> Result<(), validator::ValidationError> {
    match v {
        "ephemeral" | "persistent" => Ok(()),
        _ => Err(validator::ValidationError::new("room_type")),
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct JoinRoomReq {
    #[serde(default)]
    pub password: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn list_rooms(
    auth: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<RoomSummary>>> {
    // Rooms the user owns or is a member of, plus any persistent room (public feed later).
    let rows = sqlx::query!(
        r#"
        SELECT  r.id, r.slug, r.name, r.room_type,
                (r.password_hash IS NOT NULL) AS "locked!",
                r.created_by, r.created_at, r.last_active_at
        FROM    rooms r
        WHERE   r.created_by = $1
           OR   EXISTS (SELECT 1 FROM memberships m WHERE m.room_id = r.id AND m.user_id = $1)
        ORDER BY r.last_active_at DESC
        LIMIT 200
        "#,
        auth.id
    )
    .fetch_all(&state.db)
    .await?;

    let result = rows
        .into_iter()
        .map(|r| {
            let live = 0u32; // legado: o hub acompanha canal de voz, não sala
            RoomSummary {
                id: r.id,
                slug: r.slug.clone(),
                name: r.name,
                room_type: r.room_type,
                locked: r.locked,
                created_by: r.created_by,
                created_at: r.created_at,
                last_active_at: r.last_active_at,
                live,
            }
        })
        .collect();
    Ok(Json(result))
}

pub async fn create_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateRoomReq>,
) -> AppResult<Json<RoomSummary>> {
    body.validate()?;

    let password_hash = match &body.password {
        Some(p) if !p.is_empty() => Some(hash_password(p)?),
        _ => None,
    };

    // Retry slug generation up to 5 times on the ~impossible collision.
    let mut last_err = None;
    for _ in 0..5 {
        let slug = random_slug();
        match sqlx::query!(
            r#"
            INSERT INTO rooms (slug, name, room_type, password_hash, created_by)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, slug, name, room_type,
                      (password_hash IS NOT NULL) AS "locked!",
                      created_by, created_at, last_active_at
            "#,
            slug,
            body.name,
            body.room_type,
            password_hash,
            auth.id
        )
        .fetch_one(&state.db)
        .await
        {
            Ok(r) => {
                // Author joins automatically.
                let _ = sqlx::query!(
                    "INSERT INTO memberships (room_id, user_id, role) VALUES ($1, $2, 'host')
                     ON CONFLICT DO NOTHING",
                    r.id,
                    auth.id
                )
                .execute(&state.db)
                .await;
                return Ok(Json(RoomSummary {
                    id: r.id,
                    slug: r.slug,
                    name: r.name,
                    room_type: r.room_type,
                    locked: r.locked,
                    created_by: r.created_by,
                    created_at: r.created_at,
                    last_active_at: r.last_active_at,
                    live: 0,
                }));
            }
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => continue,
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    Err(last_err.map(AppError::from).unwrap_or(AppError::Internal(
        "could not generate unique room slug".into(),
    )))
}

pub async fn get_room(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> AppResult<Json<RoomSummary>> {
    let r = sqlx::query!(
        r#"
        SELECT  id, slug, name, room_type,
                (password_hash IS NOT NULL) AS "locked!",
                created_by, created_at, last_active_at
        FROM    rooms
        WHERE   slug = $1
        "#,
        slug
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let live = 0u32; // legado: o hub acompanha canal de voz, não sala
    Ok(Json(RoomSummary {
        id: r.id,
        slug: r.slug,
        name: r.name,
        room_type: r.room_type,
        locked: r.locked,
        created_by: r.created_by,
        created_at: r.created_at,
        last_active_at: r.last_active_at,
        live,
    }))
}

/// Validates an optional password and records membership. Used by the frontend
/// right before opening the WebSocket, so the server can reject bad passwords
/// cleanly without dropping a WS connection.
pub async fn join_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<JoinRoomReq>,
) -> AppResult<Json<RoomSummary>> {
    let r = sqlx::query!(
        r#"
        SELECT  id, slug, name, room_type, password_hash,
                (password_hash IS NOT NULL) AS "locked!",
                created_by, created_at, last_active_at
        FROM    rooms
        WHERE   slug = $1
        "#,
        slug
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if let Some(hash) = r.password_hash.as_deref() {
        let provided = body.password.unwrap_or_default();
        if !verify_password(&provided, hash)? {
            return Err(AppError::Forbidden("wrong password".into()));
        }
    }

    let _ = sqlx::query!(
        "INSERT INTO memberships (room_id, user_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
        r.id,
        auth.id
    )
    .execute(&state.db)
    .await?;

    let live = 0u32; // legado: o hub acompanha canal de voz, não sala
    Ok(Json(RoomSummary {
        id: r.id,
        slug: r.slug,
        name: r.name,
        room_type: r.room_type,
        locked: r.locked,
        created_by: r.created_by,
        created_at: r.created_at,
        last_active_at: r.last_active_at,
        live,
    }))
}
