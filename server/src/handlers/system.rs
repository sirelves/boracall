//! Non-business endpoints: health, version, stats.

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn health() -> &'static str {
    "ok"
}

pub async fn version() -> Json<Value> {
    Json(json!({
        "name": "boracall-server",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn stats(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "active_servers": state.hub.active_servers(),
    }))
}
