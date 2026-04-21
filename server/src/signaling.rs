//! Per-room broadcast hub + WebSocket handler.
//!
//! Single-node today: `tokio::sync::broadcast` fanout per slug, lazily created,
//! torn down when the last client leaves.  For multi-node, swap the inner map
//! for a `SignalBus` trait with a Redis or NATS JetStream impl (see HANDOFF).

use crate::auth::decode_token;
use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
};
use dashmap::DashMap;
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Wire protocol (JSON over text frames)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Peer-to-peer offer, targeted at a single user in the room.
    Offer { to: Uuid, sdp: String },
    /// Peer-to-peer answer, targeted at a single user in the room.
    Answer { to: Uuid, sdp: String },
    /// ICE candidate for a given peer.
    Ice { to: Uuid, candidate: serde_json::Value },
    /// Local mute state broadcast.
    Mute { muted: bool },
    /// Local "I'm speaking" pulse — coalesce on the client before emitting.
    Speaking { level: f32 },
    /// Graceful leave.
    Leave,
    /// Keepalive — server echoes `pong`.
    Ping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Full snapshot on join — who's already in the room.
    Presence { peers: Vec<Peer> },
    /// A new peer entered.
    Joined { peer: Peer },
    /// A peer disconnected.
    Left { user_id: Uuid },
    /// Relayed signaling from another peer.
    Offer { from: Uuid, sdp: String },
    Answer { from: Uuid, sdp: String },
    Ice { from: Uuid, candidate: serde_json::Value },
    /// Presence changes.
    Mute { user_id: Uuid, muted: bool },
    Speaking { user_id: Uuid, level: f32 },
    /// Errors reported back to the client.
    Error { message: String },
    Pong,
}

#[derive(Debug, Clone, Serialize)]
pub struct Peer {
    pub user_id: Uuid,
    pub display_name: Option<String>,
    pub muted: bool,
}

// ---------------------------------------------------------------------------
// Internal broadcast envelope (routed across all connected sockets in a room)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Envelope {
    /// When set, only the matching user_id should deliver. None = broadcast to all.
    target: Option<Uuid>,
    /// Sender (filtered out of their own broadcast).
    origin: Uuid,
    payload: ServerMsg,
}

// ---------------------------------------------------------------------------
// Hub — owns per-room channels.
// ---------------------------------------------------------------------------

pub struct Hub {
    rooms: DashMap<String, RoomChannel>,
}

struct RoomChannel {
    tx: broadcast::Sender<Envelope>,
    /// Current presence. Lock order: always acquire briefly.
    peers: parking_lot::RwLock<Vec<Peer>>,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            rooms: DashMap::new(),
        }
    }

    fn get_or_create(&self, slug: &str) -> broadcast::Sender<Envelope> {
        self.rooms
            .entry(slug.to_string())
            .or_insert_with(|| RoomChannel {
                tx: broadcast::channel(512).0,
                peers: parking_lot::RwLock::new(Vec::new()),
            })
            .tx
            .clone()
    }

    fn presence(&self, slug: &str) -> Vec<Peer> {
        self.rooms
            .get(slug)
            .map(|c| c.peers.read().clone())
            .unwrap_or_default()
    }

    fn add_peer(&self, slug: &str, peer: Peer) {
        if let Some(c) = self.rooms.get(slug) {
            let mut w = c.peers.write();
            w.retain(|p| p.user_id != peer.user_id); // de-dup on reconnect
            w.push(peer);
        }
    }

    fn remove_peer(&self, slug: &str, user_id: Uuid) -> bool {
        let mut drop_room = false;
        if let Some(c) = self.rooms.get(slug) {
            let mut w = c.peers.write();
            w.retain(|p| p.user_id != user_id);
            if w.is_empty() {
                drop_room = true;
            }
        }
        if drop_room {
            self.rooms.remove(slug);
        }
        drop_room
    }

    fn set_muted(&self, slug: &str, user_id: Uuid, muted: bool) {
        if let Some(c) = self.rooms.get(slug) {
            let mut w = c.peers.write();
            if let Some(p) = w.iter_mut().find(|p| p.user_id == user_id) {
                p.muted = muted;
            }
        }
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// HTTP upgrade handler — `GET /ws/rooms/:slug?token=JWT`
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: String,
}

pub async fn ws_room(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<WsQuery>,
) -> axum::response::Response {
    // Validate JWT up front; reject before upgrading if we can.
    let claims = match decode_token(&state.jwt_secret, &q.token) {
        Ok(c) => c,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return (StatusCode::UNAUTHORIZED, "bad subject").into_response(),
    };

    // Validate the room exists. Ensure membership (auto-create on join for now).
    let room = match sqlx::query!(
        "SELECT id FROM rooms WHERE slug = $1",
        slug
    )
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "room not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    // Upsert membership (idempotent).
    let _ = sqlx::query!(
        "INSERT INTO memberships (room_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        room.id,
        user_id
    )
    .execute(&state.db)
    .await;

    // Fetch display name (cheap, one query).
    let display_name = sqlx::query_scalar!(
        "SELECT display_name FROM users WHERE id = $1",
        user_id
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .flatten();

    ws.on_upgrade(move |socket| handle_socket(socket, state, slug, user_id, display_name))
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    slug: String,
    user_id: Uuid,
    display_name: Option<String>,
) {
    let tx = state.hub.get_or_create(&slug);
    let mut rx = tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Register presence BEFORE sending the initial snapshot.
    let me = Peer {
        user_id,
        display_name: display_name.clone(),
        muted: false,
    };
    state.hub.add_peer(&slug, me.clone());

    // 1) Send initial presence snapshot (who's already here).
    let peers_now = state.hub.presence(&slug);
    let snapshot = serde_json::to_string(&ServerMsg::Presence { peers: peers_now })
        .unwrap_or_else(|_| "{}".into());
    if sender.send(Message::Text(snapshot.into())).await.is_err() {
        state.hub.remove_peer(&slug, user_id);
        return;
    }

    // 2) Announce to everyone else.
    let _ = tx.send(Envelope {
        target: None,
        origin: user_id,
        payload: ServerMsg::Joined { peer: me.clone() },
    });

    // Stamp room as active now.
    let _ = sqlx::query!(
        "UPDATE rooms SET last_active_at = NOW() WHERE slug = $1",
        slug
    )
    .execute(&state.db)
    .await;

    // --- forward task: pull from broadcast, push to this socket ----------
    let forward_slug = slug.clone();
    let mut forward = tokio::spawn(async move {
        while let Ok(env) = rx.recv().await {
            // Filter: drop messages we originated (no echo) — unless targeted at us.
            if env.origin == user_id && env.target != Some(user_id) {
                continue;
            }
            // If targeted, only the target delivers.
            if let Some(t) = env.target {
                if t != user_id {
                    continue;
                }
            }
            let body = match serde_json::to_string(&env.payload) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if sender.send(Message::Text(body.into())).await.is_err() {
                break;
            }
        }
        let _ = forward_slug; // lint silencer
    });

    // --- ingest loop: read from socket, fan out via broadcast ------------
    let ingest_hub = state.hub.clone();
    let ingest_tx = tx.clone();
    let ingest_slug = slug.clone();
    let mut ingest = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    let parsed: Result<ClientMsg, _> = serde_json::from_str(&text);
                    let Ok(client_msg) = parsed else {
                        let _ = ingest_tx.send(Envelope {
                            target: Some(user_id),
                            origin: user_id,
                            payload: ServerMsg::Error {
                                message: "invalid message".into(),
                            },
                        });
                        continue;
                    };
                    let env = match client_msg {
                        ClientMsg::Offer { to, sdp } => Envelope {
                            target: Some(to),
                            origin: user_id,
                            payload: ServerMsg::Offer { from: user_id, sdp },
                        },
                        ClientMsg::Answer { to, sdp } => Envelope {
                            target: Some(to),
                            origin: user_id,
                            payload: ServerMsg::Answer { from: user_id, sdp },
                        },
                        ClientMsg::Ice { to, candidate } => Envelope {
                            target: Some(to),
                            origin: user_id,
                            payload: ServerMsg::Ice {
                                from: user_id,
                                candidate,
                            },
                        },
                        ClientMsg::Mute { muted } => {
                            ingest_hub.set_muted(&ingest_slug, user_id, muted);
                            Envelope {
                                target: None,
                                origin: user_id,
                                payload: ServerMsg::Mute { user_id, muted },
                            }
                        }
                        ClientMsg::Speaking { level } => Envelope {
                            target: None,
                            origin: user_id,
                            payload: ServerMsg::Speaking { user_id, level },
                        },
                        ClientMsg::Leave => break,
                        ClientMsg::Ping => Envelope {
                            target: Some(user_id),
                            origin: user_id,
                            payload: ServerMsg::Pong,
                        },
                    };
                    let _ = ingest_tx.send(env);
                }
                Message::Binary(_) => {} // ignore — JSON only
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Whichever task finishes first, tear both down.
    tokio::select! {
        _ = &mut forward => { ingest.abort(); }
        _ = &mut ingest  => { forward.abort(); }
    }

    // Presence cleanup + announce departure.
    let dropped = state.hub.remove_peer(&slug, user_id);
    let _ = tx.send(Envelope {
        target: None,
        origin: user_id,
        payload: ServerMsg::Left { user_id },
    });

    if dropped {
        tracing::debug!(%slug, "room emptied and reclaimed");
    }
}

// Re-export types used in the Hub.
pub use self::Peer as PeerInfo;

// Expose Hub::clone-via-Arc by just wrapping in Arc where we use it.

impl Hub {
    /// Convenience for HTTP handlers: lightweight presence snapshot without locking callers.
    pub fn slug_count(&self, slug: &str) -> usize {
        self.rooms
            .get(slug)
            .map(|r| r.peers.read().len())
            .unwrap_or(0)
    }

    pub fn active_rooms(&self) -> usize {
        self.rooms.len()
    }
}

// Make Arc<Hub> cloneable in handler arg lists via standard Arc clone.
