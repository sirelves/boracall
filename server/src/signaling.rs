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
        Path, State,
    },
    http::{header::SEC_WEBSOCKET_PROTOCOL, HeaderMap, StatusCode},
    response::IntoResponse,
};
use dashmap::DashMap;
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
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
    Ice {
        to: Uuid,
        candidate: serde_json::Value,
    },
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
    Presence {
        peers: Vec<Peer>,
    },
    /// A new peer entered.
    Joined {
        peer: Peer,
    },
    /// A peer disconnected.
    Left {
        user_id: Uuid,
    },
    /// Relayed signaling from another peer.
    Offer {
        from: Uuid,
        sdp: String,
    },
    Answer {
        from: Uuid,
        sdp: String,
    },
    Ice {
        from: Uuid,
        candidate: serde_json::Value,
    },
    /// Presence changes.
    Mute {
        user_id: Uuid,
        muted: bool,
    },
    Speaking {
        user_id: Uuid,
        level: f32,
    },
    /// Errors reported back to the client.
    Error {
        message: String,
    },
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

/// Decide se um envelope na broadcast da sala deve ser entregue ao socket de `me`.
///
/// Duas regras, nesta ordem:
/// 1. Ninguém recebe eco do que enviou — a não ser que a mensagem seja
///    endereçada a si mesmo (caso dos erros que o servidor devolve ao remetente).
/// 2. Envelope com `target` só chega em quem é o alvo; sem `target`, é broadcast.
fn should_deliver(env: &Envelope, me: Uuid) -> bool {
    if env.origin == me && env.target != Some(me) {
        return false;
    }
    match env.target {
        Some(t) => t == me,
        None => true,
    }
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
// HTTP upgrade handler — `GET /ws/rooms/:slug`
//
// Authentication: JWT travels as a WebSocket subprotocol, NOT a query param —
// query strings end up in access logs, proxy logs, and browser history, which
// leaks bearer tokens. The client does:
//
//     new WebSocket(url, ["bc.v1", "token." + jwt])
//
// and we echo back "bc.v1" as the accepted subprotocol. The server rejects
// the upgrade if the token is missing or invalid.
// ---------------------------------------------------------------------------

const WS_PROTOCOL: &str = "bc.v1";
const WS_TOKEN_PREFIX: &str = "token.";

/// Pulls the JWT out of the Sec-WebSocket-Protocol header.
/// The header can list multiple protocols comma-separated; we scan for one
/// starting with `token.` and return the suffix.
fn extract_token(headers: &HeaderMap) -> Option<String> {
    for raw in headers.get_all(SEC_WEBSOCKET_PROTOCOL).iter() {
        let Ok(s) = raw.to_str() else { continue };
        for part in s.split(',') {
            let p = part.trim();
            if let Some(tok) = p.strip_prefix(WS_TOKEN_PREFIX) {
                if !tok.is_empty() {
                    return Some(tok.to_string());
                }
            }
        }
    }
    None
}

pub async fn ws_room(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let token = match extract_token(&headers) {
        Some(t) => t,
        None => return (StatusCode::UNAUTHORIZED, "missing token subprotocol").into_response(),
    };

    // Validate JWT up front; reject before upgrading if we can.
    let claims = match decode_token(&state.jwt_secret, &token) {
        Ok(c) => c,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return (StatusCode::UNAUTHORIZED, "bad subject").into_response(),
    };

    // Load the room. Need password_hash to know if it's locked (can't trust the
    // client to have called /join first for unlocked rooms — that would break
    // the one-click invite flow — but for locked rooms, membership proves the
    // password was validated).
    let room = match sqlx::query!(
        r#"SELECT id, (password_hash IS NOT NULL) AS "locked!" FROM rooms WHERE slug = $1"#,
        slug
    )
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "room not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    // Locked rooms require a prior POST /rooms/:slug/join (which validates the
    // password and creates the membership row). The WS never auto-creates
    // membership for locked rooms — otherwise any JWT holder would bypass the
    // password by opening the WS directly.
    if room.locked {
        let is_member = sqlx::query_scalar!(
            r#"SELECT 1 AS "e!" FROM memberships WHERE room_id = $1 AND user_id = $2"#,
            room.id,
            user_id
        )
        .fetch_optional(&state.db)
        .await
        .map(|o| o.is_some())
        .unwrap_or(false);
        if !is_member {
            return (
                StatusCode::FORBIDDEN,
                "room is locked — call /rooms/{slug}/join first",
            )
                .into_response();
        }
    } else {
        // Unlocked rooms: auto-add membership on first WS connect (convenience).
        let _ = sqlx::query!(
            "INSERT INTO memberships (room_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            room.id,
            user_id
        )
        .execute(&state.db)
        .await;
    }

    // Peer cap per room. Prevents mesh topology DoS and accidental 20-person
    // "rooms" that would saturate everyone's uplink.
    // Allow the user to reconnect (same user_id already counted is deduped in
    // add_peer), so we only reject when they would actually be a NEW peer.
    let current = state.hub.slug_count(&slug);
    let already_present = state.hub.has_peer(&slug, user_id);
    if !already_present && current >= state.max_peers_per_room {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            format!("room full (max {} peers)", state.max_peers_per_room),
        )
            .into_response();
    }

    // Fetch display name (cheap, one query).
    let display_name = sqlx::query_scalar!("SELECT display_name FROM users WHERE id = $1", user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .flatten();

    // Echo the accepted subprotocol back — the browser's WebSocket client
    // rejects the handshake if we don't confirm one of the protocols it offered.
    ws.protocols([WS_PROTOCOL])
        .on_upgrade(move |socket| handle_socket(socket, state, slug, user_id, display_name))
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
    let mut forward = tokio::spawn(async move {
        while let Ok(env) = rx.recv().await {
            if !should_deliver(&env, user_id) {
                continue;
            }
            let body = match serde_json::to_string(&env.payload) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if sender.send(Message::Text(body.into())).await.is_err() {
                break;
            }
        }
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

// Expose Hub::clone-via-Arc by just wrapping in Arc where we use it.

impl Hub {
    /// Convenience for HTTP handlers: lightweight presence snapshot without locking callers.
    pub fn slug_count(&self, slug: &str) -> usize {
        self.rooms
            .get(slug)
            .map(|r| r.peers.read().len())
            .unwrap_or(0)
    }

    /// True if `user_id` is currently listed as a peer in `slug`.
    /// Used to distinguish "new peer would exceed cap" from "same user reconnecting".
    pub fn has_peer(&self, slug: &str, user_id: Uuid) -> bool {
        self.rooms
            .get(slug)
            .map(|r| r.peers.read().iter().any(|p| p.user_id == user_id))
            .unwrap_or(false)
    }

    pub fn active_rooms(&self) -> usize {
        self.rooms.len()
    }
}

// Make Arc<Hub> cloneable in handler arg lists via standard Arc clone.

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn peer(id: Uuid) -> Peer {
        Peer {
            user_id: id,
            display_name: Some("fulano".into()),
            muted: false,
        }
    }

    fn envelope(origin: Uuid, target: Option<Uuid>) -> Envelope {
        Envelope {
            target,
            origin,
            payload: ServerMsg::Left { user_id: origin },
        }
    }

    fn headers_with(protocols: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for p in protocols {
            h.append(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_str(p).unwrap());
        }
        h
    }

    // --- autenticação no handshake ----------------------------------------

    #[test]
    fn token_sai_do_subprotocol() {
        let h = headers_with(&["bc.v1, token.abc123"]);
        assert_eq!(extract_token(&h).as_deref(), Some("abc123"));
    }

    #[test]
    fn token_e_encontrado_em_qualquer_posicao_da_lista() {
        let h = headers_with(&["token.xyz, bc.v1"]);
        assert_eq!(extract_token(&h).as_deref(), Some("xyz"));
    }

    #[test]
    fn token_e_encontrado_em_headers_repetidos() {
        // Cliente pode mandar Sec-WebSocket-Protocol em linhas separadas.
        let h = headers_with(&["bc.v1", "token.dois"]);
        assert_eq!(extract_token(&h).as_deref(), Some("dois"));
    }

    #[test]
    fn sem_token_no_subprotocol_retorna_none() {
        assert_eq!(extract_token(&headers_with(&["bc.v1"])), None);
        assert_eq!(extract_token(&HeaderMap::new()), None);
    }

    #[test]
    fn prefixo_token_vazio_nao_conta_como_token() {
        // "token." sozinho não é credencial — não pode virar Some("").
        assert_eq!(extract_token(&headers_with(&["bc.v1, token."])), None);
    }

    #[test]
    fn jwt_com_pontos_sobrevive_ao_parse() {
        // JWT tem 3 partes separadas por ponto; o prefixo é "token." e o resto
        // precisa voltar inteiro.
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.assinatura-aqui";
        let h = headers_with(&[&format!("bc.v1, token.{jwt}")]);
        assert_eq!(extract_token(&h).as_deref(), Some(jwt));
    }

    // --- roteamento de mensagens ------------------------------------------

    #[test]
    fn broadcast_chega_em_todo_mundo_menos_em_quem_enviou() {
        let eu = Uuid::new_v4();
        let outro = Uuid::new_v4();
        let env = envelope(outro, None);

        assert!(
            should_deliver(&env, eu),
            "broadcast de terceiro deve chegar"
        );
        assert!(
            !should_deliver(&envelope(eu, None), eu),
            "ninguém recebe eco do próprio broadcast"
        );
    }

    #[test]
    fn mensagem_direcionada_so_chega_no_alvo() {
        let alvo = Uuid::new_v4();
        let outro = Uuid::new_v4();
        let remetente = Uuid::new_v4();
        let env = envelope(remetente, Some(alvo));

        assert!(should_deliver(&env, alvo));
        assert!(
            !should_deliver(&env, outro),
            "SDP/ICE de outro par não pode vazar pra sala inteira"
        );
    }

    #[test]
    fn mensagem_do_servidor_pro_proprio_remetente_chega() {
        // Caso do ServerMsg::Error: origin == target == eu. A regra do "sem eco"
        // não pode engolir essa.
        let eu = Uuid::new_v4();
        assert!(should_deliver(&envelope(eu, Some(eu)), eu));
    }

    // --- Hub / presença ----------------------------------------------------

    #[test]
    fn sala_nasce_no_get_or_create_e_some_quando_esvazia() {
        let hub = Hub::new();
        let id = Uuid::new_v4();

        hub.get_or_create("sala1");
        hub.add_peer("sala1", peer(id));
        assert_eq!(hub.active_rooms(), 1);
        assert_eq!(hub.slug_count("sala1"), 1);

        assert!(hub.remove_peer("sala1", id), "último a sair derruba a sala");
        assert_eq!(hub.active_rooms(), 0);
        assert_eq!(hub.slug_count("sala1"), 0);
    }

    #[test]
    fn sala_com_gente_dentro_nao_e_derrubada() {
        let hub = Hub::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        hub.get_or_create("sala2");
        hub.add_peer("sala2", peer(a));
        hub.add_peer("sala2", peer(b));

        assert!(!hub.remove_peer("sala2", a), "ainda tem gente na sala");
        assert_eq!(hub.slug_count("sala2"), 1);
        assert!(hub.has_peer("sala2", b));
        assert!(!hub.has_peer("sala2", a));
    }

    #[test]
    fn reconexao_do_mesmo_usuario_nao_duplica_presenca() {
        let hub = Hub::new();
        let id = Uuid::new_v4();
        hub.get_or_create("sala3");

        hub.add_peer("sala3", peer(id));
        hub.add_peer("sala3", peer(id));

        assert_eq!(
            hub.slug_count("sala3"),
            1,
            "reconectar não pode contar duas vezes — isso furaria o cap de peers"
        );
    }

    #[test]
    fn add_peer_em_sala_inexistente_e_no_op() {
        // add_peer depende do get_or_create ter rodado antes (é o que o
        // handshake faz). Sem isso, some silenciosamente.
        let hub = Hub::new();
        hub.add_peer("nunca-criada", peer(Uuid::new_v4()));
        assert_eq!(hub.slug_count("nunca-criada"), 0);
        assert_eq!(hub.active_rooms(), 0);
    }

    #[test]
    fn set_muted_reflete_no_snapshot_de_presenca() {
        let hub = Hub::new();
        let id = Uuid::new_v4();
        hub.get_or_create("sala4");
        hub.add_peer("sala4", peer(id));

        hub.set_muted("sala4", id, true);
        let snapshot = hub.presence("sala4");
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot[0].muted);
    }

    #[test]
    fn salas_sao_isoladas_entre_si() {
        let hub = Hub::new();
        let a = Uuid::new_v4();
        hub.get_or_create("sala-a");
        hub.get_or_create("sala-b");
        hub.add_peer("sala-a", peer(a));

        assert!(hub.has_peer("sala-a", a));
        assert!(!hub.has_peer("sala-b", a));
        assert_eq!(hub.active_rooms(), 2);
    }
}
