//! Hub de broadcast por servidor + handler WebSocket.
//!
//! **Uma conexão por servidor**, não por canal. O usuário precisa, ao mesmo
//! tempo, receber mensagem de texto de todos os canais que enxerga e ver quem
//! está em cada canal de voz — mas fica dentro de no máximo um canal de voz.
//! Uma conexão por canal seria N conexões por usuário.
//!
//! Single-node hoje: um `tokio::sync::broadcast` por servidor, criado sob
//! demanda e recolhido quando o último socket sai. Pra multi-nó, trocar o mapa
//! interno por um trait `SignalBus` com impl NATS ou Redis (ver ARCHITECTURE).

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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Protocolo de fio (JSON em frames de texto)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Entra num canal de voz. Sair do anterior é implícito — o usuário está em
    /// no máximo um canal de voz por servidor.
    JoinVoice { channel_id: Uuid },
    /// Sai do canal de voz atual, sem derrubar a conexão com o servidor.
    LeaveVoice,
    /// Offer P2P, endereçada a um usuário do mesmo canal de voz.
    Offer { to: Uuid, sdp: String },
    /// Answer P2P, endereçada a um usuário do mesmo canal de voz.
    Answer { to: Uuid, sdp: String },
    /// Candidato ICE pra um par específico.
    Ice {
        to: Uuid,
        candidate: serde_json::Value,
    },
    /// Estado local de mudo.
    Mute { muted: bool },
    /// Pulso de "estou falando" — o cliente agrupa antes de emitir.
    Speaking { level: f32 },
    /// "Fulano está digitando" num canal de texto.
    Typing { channel_id: Uuid },
    /// Saída limpa da conexão.
    Leave,
    /// Keepalive — o servidor responde `pong`.
    Ping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Snapshot mandado logo após conectar: quem está em cada canal de voz.
    VoiceState {
        channels: Vec<VoiceChannelState>,
    },
    /// Presença completa de um canal de voz, após alguém entrar ou sair.
    VoicePresence {
        channel_id: Uuid,
        peers: Vec<Peer>,
    },
    VoiceJoined {
        channel_id: Uuid,
        peer: Peer,
    },
    VoiceLeft {
        channel_id: Uuid,
        user_id: Uuid,
    },
    /// Signaling repassado de outro par.
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
    Mute {
        channel_id: Uuid,
        user_id: Uuid,
        muted: bool,
    },
    Speaking {
        channel_id: Uuid,
        user_id: Uuid,
        level: f32,
    },
    /// Mensagem nova num canal de texto. Publicada pelo handler HTTP que
    /// persistiu — o WebSocket só transporta.
    Message {
        channel_id: Uuid,
        message: serde_json::Value,
    },
    /// Mensagem editada ou apagada, pra quem está com o canal aberto.
    MessageUpdated {
        channel_id: Uuid,
        message: serde_json::Value,
    },
    MessageDeleted {
        channel_id: Uuid,
        message_id: Uuid,
    },
    Typing {
        channel_id: Uuid,
        user_id: Uuid,
    },
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

#[derive(Debug, Clone, Serialize)]
pub struct VoiceChannelState {
    pub channel_id: Uuid,
    pub peers: Vec<Peer>,
}

// ---------------------------------------------------------------------------
// Envelope interno (roteado entre todos os sockets do servidor)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Envelope {
    /// Quando preenchido, só o usuário correspondente entrega. None = todos.
    target: Option<Uuid>,
    /// Quando preenchido, só quem está NESTE canal de voz entrega.
    scope: Option<Uuid>,
    /// Quem enviou (filtrado do próprio broadcast).
    origin: Uuid,
    payload: ServerMsg,
}

/// Decide se um envelope na broadcast do servidor deve ser entregue ao socket de
/// `me`, que no momento está no canal de voz `my_voice` (ou em nenhum).
///
/// Três regras, nesta ordem:
/// 1. Ninguém recebe eco do que enviou — a não ser que a mensagem seja
///    endereçada a si mesmo (caso dos erros que o servidor devolve ao remetente).
/// 2. Envelope com `target` só chega em quem é o alvo.
/// 3. Envelope com `scope` só chega em quem está naquele canal de voz. Sem
///    `scope`, vale pro servidor inteiro (mensagem de texto, por exemplo).
fn should_deliver(env: &Envelope, me: Uuid, my_voice: Option<Uuid>) -> bool {
    if env.origin == me && env.target != Some(me) {
        return false;
    }
    if let Some(t) = env.target {
        return t == me;
    }
    match env.scope {
        Some(ch) => my_voice == Some(ch),
        None => true,
    }
}

// ---------------------------------------------------------------------------
// Hub — um canal de broadcast por servidor, presença de voz por canal.
// ---------------------------------------------------------------------------

pub struct Hub {
    servers: DashMap<String, ServerChannel>,
}

struct ServerChannel {
    tx: broadcast::Sender<Envelope>,
    /// channel_id → quem está falando ali. Lock sempre por pouco tempo.
    voice: parking_lot::RwLock<HashMap<Uuid, Vec<Peer>>>,
}

/// Motivo de recusa ao entrar num canal de voz.
#[derive(Debug, PartialEq, Eq)]
pub enum VoiceJoinError {
    /// A mesh P2P satura acima de ~4 pessoas; o cap é o guarda-corpo até o SFU.
    ChannelFull { max: usize },
}

impl Hub {
    pub fn new() -> Self {
        Self {
            servers: DashMap::new(),
        }
    }

    fn get_or_create(&self, server: &str) -> broadcast::Sender<Envelope> {
        self.servers
            .entry(server.to_string())
            .or_insert_with(|| ServerChannel {
                tx: broadcast::channel(512).0,
                voice: parking_lot::RwLock::new(HashMap::new()),
            })
            .tx
            .clone()
    }

    /// Entra (ou troca) de canal de voz. Devolve a presença nova do canal.
    ///
    /// Sair do canal anterior é implícito: no Discord você não fica em dois
    /// canais de voz ao mesmo tempo, e deixar presença órfã pra trás faria o
    /// contador de "ao vivo" mentir.
    pub fn join_voice(
        &self,
        server: &str,
        channel_id: Uuid,
        peer: Peer,
        max_peers: usize,
    ) -> Result<(Vec<Peer>, Option<Uuid>), VoiceJoinError> {
        let Some(sc) = self.servers.get(server) else {
            return Ok((vec![peer], None));
        };
        let mut voice = sc.voice.write();

        // Tira de qualquer outro canal antes de entrar no novo.
        let mut left_from = None;
        for (cid, peers) in voice.iter_mut() {
            if *cid != channel_id && peers.iter().any(|p| p.user_id == peer.user_id) {
                peers.retain(|p| p.user_id != peer.user_id);
                left_from = Some(*cid);
            }
        }

        let entry = voice.entry(channel_id).or_default();
        let already_here = entry.iter().any(|p| p.user_id == peer.user_id);

        // Cap conta gente distinta: reconectar não pode ser barrado por si mesmo.
        if !already_here && entry.len() >= max_peers {
            return Err(VoiceJoinError::ChannelFull { max: max_peers });
        }

        entry.retain(|p| p.user_id != peer.user_id); // de-dup na reconexão
        entry.push(peer);
        let presence = entry.clone();

        voice.retain(|_, peers| !peers.is_empty());
        Ok((presence, left_from))
    }

    /// Sai do canal de voz atual. Devolve (canal, presença restante).
    pub fn leave_voice(&self, server: &str, user_id: Uuid) -> Option<(Uuid, Vec<Peer>)> {
        let sc = self.servers.get(server)?;
        let mut voice = sc.voice.write();

        let found = voice
            .iter()
            .find(|(_, peers)| peers.iter().any(|p| p.user_id == user_id))
            .map(|(cid, _)| *cid)?;

        let peers = voice.get_mut(&found)?;
        peers.retain(|p| p.user_id != user_id);
        let remaining = peers.clone();

        voice.retain(|_, peers| !peers.is_empty());
        Some((found, remaining))
    }

    /// Em qual canal de voz o usuário está, se estiver em algum.
    pub fn voice_channel_of(&self, server: &str, user_id: Uuid) -> Option<Uuid> {
        let sc = self.servers.get(server)?;
        let voice = sc.voice.read();
        voice
            .iter()
            .find(|(_, peers)| peers.iter().any(|p| p.user_id == user_id))
            .map(|(cid, _)| *cid)
    }

    pub fn voice_presence(&self, server: &str, channel_id: Uuid) -> Vec<Peer> {
        self.servers
            .get(server)
            .and_then(|sc| sc.voice.read().get(&channel_id).cloned())
            .unwrap_or_default()
    }

    /// Snapshot de todos os canais de voz com gente dentro.
    pub fn voice_snapshot(&self, server: &str) -> Vec<VoiceChannelState> {
        self.servers
            .get(server)
            .map(|sc| {
                sc.voice
                    .read()
                    .iter()
                    .map(|(cid, peers)| VoiceChannelState {
                        channel_id: *cid,
                        peers: peers.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Atualiza o mudo e devolve o canal onde a pessoa está.
    pub fn set_muted(&self, server: &str, user_id: Uuid, muted: bool) -> Option<Uuid> {
        let sc = self.servers.get(server)?;
        let mut voice = sc.voice.write();
        for (cid, peers) in voice.iter_mut() {
            if let Some(p) = peers.iter_mut().find(|p| p.user_id == user_id) {
                p.muted = muted;
                return Some(*cid);
            }
        }
        None
    }

    /// Quantas pessoas no canal de voz agora — usado pelo detalhe do servidor.
    pub fn live_in(&self, server: &str, channel_id: Uuid) -> usize {
        self.servers
            .get(server)
            .map(|sc| sc.voice.read().get(&channel_id).map_or(0, |p| p.len()))
            .unwrap_or(0)
    }

    /// Publica um evento no servidor inteiro. Usado pelos handlers HTTP que
    /// persistem (mensagem nova, edição, remoção) — a escrita é HTTP, o aviso
    /// em tempo real é WebSocket.
    ///
    /// No-op quando ninguém está conectado naquele servidor: sem socket, não há
    /// pra quem avisar, e criar o canal à toa só vazaria memória.
    pub fn publish(&self, server: &str, origin: Uuid, payload: ServerMsg) {
        if let Some(sc) = self.servers.get(server) {
            let _ = sc.tx.send(Envelope {
                target: None,
                scope: None,
                origin,
                payload,
            });
        }
    }

    /// Recolhe o servidor quando o último socket sai.
    fn reclaim_if_idle(&self, server: &str) -> bool {
        let idle = self
            .servers
            .get(server)
            .map(|sc| sc.tx.receiver_count() == 0 && sc.voice.read().is_empty())
            .unwrap_or(false);
        if idle {
            self.servers.remove(server);
        }
        idle
    }

    pub fn active_servers(&self) -> usize {
        self.servers.len()
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Handler de upgrade — `GET /ws/servers/{slug}`
//
// Autenticação: o JWT viaja como subprotocol do WebSocket, NÃO como query
// param — query string acaba em log de acesso, log de proxy e histórico de
// browser, o que vaza o bearer token. O cliente faz:
//
//     new WebSocket(url, ["bc.v1", "token." + jwt])
//
// e o servidor devolve "bc.v1" como subprotocol aceito.
// ---------------------------------------------------------------------------

const WS_PROTOCOL: &str = "bc.v1";
const WS_TOKEN_PREFIX: &str = "token.";

/// Extrai o JWT do header Sec-WebSocket-Protocol. O header pode listar vários
/// protocolos separados por vírgula; varremos procurando o que começa com
/// `token.` e devolvemos o sufixo.
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

pub async fn ws_server(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let token = match extract_token(&headers) {
        Some(t) => t,
        None => return (StatusCode::UNAUTHORIZED, "missing token subprotocol").into_response(),
    };

    let claims = match decode_token(&state.jwt_secret, &token) {
        Ok(c) => c,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return (StatusCode::UNAUTHORIZED, "bad subject").into_response(),
    };

    // Só membro conecta. 404 e não 403 pelo mesmo motivo do HTTP: responder
    // "proibido" confirmaria que o servidor existe pra quem não faz parte dele.
    let server = sqlx::query!(
        r#"
        SELECT s.id, (m.user_id IS NOT NULL) AS "is_member!"
        FROM servers s
        LEFT JOIN server_members m ON m.server_id = s.id AND m.user_id = $2
        WHERE s.slug = $1
        "#,
        slug,
        user_id
    )
    .fetch_optional(&state.db)
    .await;

    let server = match server {
        Ok(Some(r)) if r.is_member => r,
        Ok(_) => return (StatusCode::NOT_FOUND, "server not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    let display_name = sqlx::query_scalar!("SELECT display_name FROM users WHERE id = $1", user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .flatten();

    ws.protocols([WS_PROTOCOL]).on_upgrade(move |socket| {
        handle_socket(socket, state, slug, server.id, user_id, display_name)
    })
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    server_slug: String,
    server_id: Uuid,
    user_id: Uuid,
    display_name: Option<String>,
) {
    let tx = state.hub.get_or_create(&server_slug);
    let mut rx = tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Em qual canal de voz este socket está. Compartilhado entre a task que
    // ingere (que muda) e a que entrega (que filtra por escopo).
    let my_voice: Arc<parking_lot::RwLock<Option<Uuid>>> = Arc::new(parking_lot::RwLock::new(None));

    // 1) Snapshot: quem está em cada canal de voz do servidor.
    let snapshot = serde_json::to_string(&ServerMsg::VoiceState {
        channels: state.hub.voice_snapshot(&server_slug),
    })
    .unwrap_or_else(|_| "{}".into());
    if sender.send(Message::Text(snapshot.into())).await.is_err() {
        return;
    }

    // --- entrega: broadcast do servidor → este socket ---------------------
    let forward_voice = my_voice.clone();
    let mut forward = tokio::spawn(async move {
        while let Ok(env) = rx.recv().await {
            let current = *forward_voice.read();
            if !should_deliver(&env, user_id, current) {
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

    // --- ingestão: este socket → broadcast do servidor --------------------
    let ingest_state = state.clone();
    let ingest_tx = tx.clone();
    let ingest_slug = server_slug.clone();
    let ingest_voice = my_voice.clone();
    let ingest_name = display_name.clone();
    let mut ingest = tokio::spawn(async move {
        let hub = &ingest_state.hub;

        let err_to_self = |msg: &str| Envelope {
            target: Some(user_id),
            scope: None,
            origin: user_id,
            payload: ServerMsg::Error {
                message: msg.to_string(),
            },
        };

        while let Some(Ok(msg)) = receiver.next().await {
            let Message::Text(text) = msg else {
                match msg {
                    Message::Close(_) => break,
                    _ => continue, // binário e frames de controle: ignorados
                }
            };

            let Ok(client_msg) = serde_json::from_str::<ClientMsg>(&text) else {
                let _ = ingest_tx.send(err_to_self("invalid message"));
                continue;
            };

            match client_msg {
                ClientMsg::JoinVoice { channel_id } => {
                    // O canal precisa existir, ser deste servidor e ser de voz —
                    // senão dá pra entrar num canal de texto ou de outro servidor
                    // só mandando o uuid.
                    let ok = sqlx::query_scalar!(
                        r#"SELECT 1 AS "e!" FROM channels
                           WHERE id = $1 AND server_id = $2 AND kind = 'voice'"#,
                        channel_id,
                        server_id
                    )
                    .fetch_optional(&ingest_state.db)
                    .await
                    .ok()
                    .flatten()
                    .is_some();

                    if !ok {
                        let _ = ingest_tx.send(err_to_self("canal de voz inválido"));
                        continue;
                    }

                    let peer = Peer {
                        user_id,
                        display_name: ingest_name.clone(),
                        muted: false,
                    };
                    match hub.join_voice(
                        &ingest_slug,
                        channel_id,
                        peer.clone(),
                        ingest_state.max_peers_per_channel,
                    ) {
                        Err(VoiceJoinError::ChannelFull { max }) => {
                            let _ = ingest_tx
                                .send(err_to_self(&format!("canal de voz cheio (máximo {max})")));
                            continue;
                        }
                        Ok((presence, left_from)) => {
                            *ingest_voice.write() = Some(channel_id);

                            // Se trocou de canal, avisa o canal anterior.
                            if let Some(old) = left_from {
                                let remaining = hub.voice_presence(&ingest_slug, old);
                                let _ = ingest_tx.send(Envelope {
                                    target: None,
                                    scope: None,
                                    origin: user_id,
                                    payload: ServerMsg::VoiceLeft {
                                        channel_id: old,
                                        user_id,
                                    },
                                });
                                let _ = ingest_tx.send(Envelope {
                                    target: None,
                                    scope: None,
                                    origin: user_id,
                                    payload: ServerMsg::VoicePresence {
                                        channel_id: old,
                                        peers: remaining,
                                    },
                                });
                            }

                            // Presença vai pro servidor inteiro (a sidebar
                            // mostra quem está em cada canal), não só pro canal.
                            let _ = ingest_tx.send(Envelope {
                                target: None,
                                scope: None,
                                origin: user_id,
                                payload: ServerMsg::VoiceJoined { channel_id, peer },
                            });
                            let _ = ingest_tx.send(Envelope {
                                target: None,
                                scope: None,
                                origin: user_id,
                                payload: ServerMsg::VoicePresence {
                                    channel_id,
                                    peers: presence.clone(),
                                },
                            });
                            // …e o próprio recebe a lista pra montar a mesh.
                            let _ = ingest_tx.send(Envelope {
                                target: Some(user_id),
                                scope: None,
                                origin: Uuid::nil(),
                                payload: ServerMsg::VoicePresence {
                                    channel_id,
                                    peers: presence,
                                },
                            });
                        }
                    }
                }

                ClientMsg::LeaveVoice => {
                    if let Some((channel_id, remaining)) = hub.leave_voice(&ingest_slug, user_id) {
                        *ingest_voice.write() = None;
                        let _ = ingest_tx.send(Envelope {
                            target: None,
                            scope: None,
                            origin: user_id,
                            payload: ServerMsg::VoiceLeft {
                                channel_id,
                                user_id,
                            },
                        });
                        let _ = ingest_tx.send(Envelope {
                            target: None,
                            scope: None,
                            origin: user_id,
                            payload: ServerMsg::VoicePresence {
                                channel_id,
                                peers: remaining,
                            },
                        });
                    }
                }

                // SDP e ICE só trafegam entre pares do MESMO canal de voz. Sem
                // essa checagem, qualquer membro do servidor mandaria offer pra
                // qualquer outro e forçaria uma negociação fora do canal.
                ClientMsg::Offer { to, sdp } => {
                    if same_voice_channel(hub, &ingest_slug, user_id, to) {
                        let _ = ingest_tx.send(Envelope {
                            target: Some(to),
                            scope: None,
                            origin: user_id,
                            payload: ServerMsg::Offer { from: user_id, sdp },
                        });
                    } else {
                        let _ = ingest_tx.send(err_to_self("par não está no seu canal de voz"));
                    }
                }
                ClientMsg::Answer { to, sdp } => {
                    if same_voice_channel(hub, &ingest_slug, user_id, to) {
                        let _ = ingest_tx.send(Envelope {
                            target: Some(to),
                            scope: None,
                            origin: user_id,
                            payload: ServerMsg::Answer { from: user_id, sdp },
                        });
                    } else {
                        let _ = ingest_tx.send(err_to_self("par não está no seu canal de voz"));
                    }
                }
                ClientMsg::Ice { to, candidate } => {
                    if same_voice_channel(hub, &ingest_slug, user_id, to) {
                        let _ = ingest_tx.send(Envelope {
                            target: Some(to),
                            scope: None,
                            origin: user_id,
                            payload: ServerMsg::Ice {
                                from: user_id,
                                candidate,
                            },
                        });
                    }
                    // ICE fora do canal é descartado em silêncio: chega aos
                    // borbotões e um erro por candidato viraria enxurrada.
                }

                ClientMsg::Mute { muted } => {
                    if let Some(channel_id) = hub.set_muted(&ingest_slug, user_id, muted) {
                        let _ = ingest_tx.send(Envelope {
                            target: None,
                            scope: None,
                            origin: user_id,
                            payload: ServerMsg::Mute {
                                channel_id,
                                user_id,
                                muted,
                            },
                        });
                    }
                }

                ClientMsg::Speaking { level } => {
                    let current = *ingest_voice.read();
                    if let Some(channel_id) = current {
                        // Escopado no canal: quem está em outro canal não precisa
                        // receber pulso de fala a 10Hz de gente que não ouve.
                        let _ = ingest_tx.send(Envelope {
                            target: None,
                            scope: Some(channel_id),
                            origin: user_id,
                            payload: ServerMsg::Speaking {
                                channel_id,
                                user_id,
                                level,
                            },
                        });
                    }
                }

                ClientMsg::Typing { channel_id } => {
                    let _ = ingest_tx.send(Envelope {
                        target: None,
                        scope: None,
                        origin: user_id,
                        payload: ServerMsg::Typing {
                            channel_id,
                            user_id,
                        },
                    });
                }

                ClientMsg::Leave => break,
                ClientMsg::Ping => {
                    let _ = ingest_tx.send(Envelope {
                        target: Some(user_id),
                        scope: None,
                        origin: Uuid::nil(),
                        payload: ServerMsg::Pong,
                    });
                }
            }
        }
    });

    tokio::select! {
        _ = &mut forward => { ingest.abort(); }
        _ = &mut ingest  => { forward.abort(); }
    }

    // Limpeza: sair do canal de voz e avisar quem ficou.
    if let Some((channel_id, remaining)) = state.hub.leave_voice(&server_slug, user_id) {
        let _ = tx.send(Envelope {
            target: None,
            scope: None,
            origin: user_id,
            payload: ServerMsg::VoiceLeft {
                channel_id,
                user_id,
            },
        });
        let _ = tx.send(Envelope {
            target: None,
            scope: None,
            origin: user_id,
            payload: ServerMsg::VoicePresence {
                channel_id,
                peers: remaining,
            },
        });
    }

    drop(tx);
    if state.hub.reclaim_if_idle(&server_slug) {
        tracing::debug!(%server_slug, "servidor sem ninguém conectado, recolhido");
    }
}

/// Os dois estão no mesmo canal de voz?
fn same_voice_channel(hub: &Hub, server: &str, a: Uuid, b: Uuid) -> bool {
    match hub.voice_channel_of(server, a) {
        Some(ch) => hub.voice_channel_of(server, b) == Some(ch),
        None => false,
    }
}

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

    fn envelope(origin: Uuid, target: Option<Uuid>, scope: Option<Uuid>) -> Envelope {
        Envelope {
            target,
            scope,
            origin,
            payload: ServerMsg::Pong,
        }
    }

    fn headers_with(protocols: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for p in protocols {
            h.append(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_str(p).unwrap());
        }
        h
    }

    /// Hub com um servidor já criado — `join_voice` exige que ele exista, que é
    /// o que o handshake faz antes de qualquer coisa.
    fn hub_with(server: &str) -> Hub {
        let hub = Hub::new();
        hub.get_or_create(server);
        hub
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
        assert_eq!(extract_token(&headers_with(&["bc.v1, token."])), None);
    }

    #[test]
    fn jwt_com_pontos_sobrevive_ao_parse() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.assinatura-aqui";
        let h = headers_with(&[&format!("bc.v1, token.{jwt}")]);
        assert_eq!(extract_token(&h).as_deref(), Some(jwt));
    }

    // --- roteamento de mensagens ------------------------------------------

    #[test]
    fn broadcast_do_servidor_chega_em_todo_mundo_menos_em_quem_enviou() {
        let eu = Uuid::new_v4();
        let outro = Uuid::new_v4();

        assert!(should_deliver(&envelope(outro, None, None), eu, None));
        assert!(
            !should_deliver(&envelope(eu, None, None), eu, None),
            "ninguém recebe eco do próprio broadcast"
        );
    }

    #[test]
    fn mensagem_direcionada_so_chega_no_alvo() {
        let alvo = Uuid::new_v4();
        let outro = Uuid::new_v4();
        let env = envelope(Uuid::new_v4(), Some(alvo), None);

        assert!(should_deliver(&env, alvo, None));
        assert!(
            !should_deliver(&env, outro, None),
            "SDP/ICE de outro par não pode vazar"
        );
    }

    #[test]
    fn mensagem_do_servidor_pro_proprio_remetente_chega() {
        let eu = Uuid::new_v4();
        assert!(should_deliver(&envelope(eu, Some(eu), None), eu, None));
    }

    #[test]
    fn evento_escopado_so_chega_em_quem_esta_naquele_canal_de_voz() {
        let canal_a = Uuid::new_v4();
        let canal_b = Uuid::new_v4();
        let quem_enviou = Uuid::new_v4();
        let eu = Uuid::new_v4();
        let env = envelope(quem_enviou, None, Some(canal_a));

        assert!(should_deliver(&env, eu, Some(canal_a)), "estou no canal");
        assert!(
            !should_deliver(&env, eu, Some(canal_b)),
            "pulso de fala do canal A não pode chegar em quem está no B"
        );
        assert!(
            !should_deliver(&env, eu, None),
            "quem não está em canal nenhum não recebe evento de voz"
        );
    }

    #[test]
    fn mensagem_de_texto_chega_em_todo_mundo_do_servidor() {
        // Sem escopo: quem está numa call e quem não está recebem igual.
        let env = envelope(Uuid::new_v4(), None, None);
        let eu = Uuid::new_v4();
        assert!(should_deliver(&env, eu, None));
        assert!(should_deliver(&env, eu, Some(Uuid::new_v4())));
    }

    // --- presença de voz ---------------------------------------------------

    #[test]
    fn entrar_no_canal_de_voz_aparece_na_presenca() {
        let hub = hub_with("srv");
        let canal = Uuid::new_v4();
        let eu = Uuid::new_v4();

        let (presenca, saiu_de) = hub.join_voice("srv", canal, peer(eu), 6).unwrap();
        assert_eq!(presenca.len(), 1);
        assert!(saiu_de.is_none());
        assert_eq!(hub.live_in("srv", canal), 1);
        assert_eq!(hub.voice_channel_of("srv", eu), Some(canal));
    }

    #[test]
    fn usuario_fica_em_um_canal_de_voz_so() {
        let hub = hub_with("srv");
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let eu = Uuid::new_v4();

        hub.join_voice("srv", a, peer(eu), 6).unwrap();
        let (_, saiu_de) = hub.join_voice("srv", b, peer(eu), 6).unwrap();

        assert_eq!(
            saiu_de,
            Some(a),
            "trocar de canal precisa avisar o anterior"
        );
        assert_eq!(hub.live_in("srv", a), 0, "presença órfã no canal antigo");
        assert_eq!(hub.live_in("srv", b), 1);
        assert_eq!(hub.voice_channel_of("srv", eu), Some(b));
    }

    #[test]
    fn reconexao_no_mesmo_canal_nao_duplica_nem_estoura_o_cap() {
        let hub = hub_with("srv");
        let canal = Uuid::new_v4();
        let eu = Uuid::new_v4();

        // Cap 1 e o mesmo usuário entrando duas vezes: a segunda é reconexão,
        // não gente nova — não pode ser barrada por si mesma.
        hub.join_voice("srv", canal, peer(eu), 1).unwrap();
        let (presenca, _) = hub
            .join_voice("srv", canal, peer(eu), 1)
            .expect("reconexão do mesmo usuário não pode bater no cap");
        assert_eq!(presenca.len(), 1);
        assert_eq!(hub.live_in("srv", canal), 1);
    }

    #[test]
    fn cap_por_canal_barra_o_proximo() {
        let hub = hub_with("srv");
        let canal = Uuid::new_v4();

        for _ in 0..2 {
            hub.join_voice("srv", canal, peer(Uuid::new_v4()), 2)
                .unwrap();
        }
        let err = hub
            .join_voice("srv", canal, peer(Uuid::new_v4()), 2)
            .expect_err("terceiro tem que ser barrado");
        assert_eq!(err, VoiceJoinError::ChannelFull { max: 2 });
        assert_eq!(hub.live_in("srv", canal), 2);
    }

    #[test]
    fn cap_e_por_canal_e_nao_por_servidor() {
        let hub = hub_with("srv");
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        hub.join_voice("srv", a, peer(Uuid::new_v4()), 1).unwrap();
        // Canal B tem o próprio cap — encher o A não pode fechar o servidor.
        hub.join_voice("srv", b, peer(Uuid::new_v4()), 1)
            .expect("outro canal tem cap próprio");
        assert_eq!(hub.live_in("srv", a), 1);
        assert_eq!(hub.live_in("srv", b), 1);
    }

    #[test]
    fn sair_do_canal_libera_vaga_e_some_do_snapshot() {
        let hub = hub_with("srv");
        let canal = Uuid::new_v4();
        let eu = Uuid::new_v4();
        let outro = Uuid::new_v4();

        hub.join_voice("srv", canal, peer(eu), 6).unwrap();
        hub.join_voice("srv", canal, peer(outro), 6).unwrap();

        let (saiu, restantes) = hub.leave_voice("srv", eu).expect("estava no canal");
        assert_eq!(saiu, canal);
        assert_eq!(restantes.len(), 1);
        assert_eq!(hub.voice_channel_of("srv", eu), None);

        // Último saindo esvazia o canal e ele some do snapshot.
        hub.leave_voice("srv", outro).unwrap();
        assert_eq!(hub.live_in("srv", canal), 0);
        assert!(hub.voice_snapshot("srv").is_empty());
    }

    #[test]
    fn sair_sem_estar_em_canal_nenhum_nao_quebra() {
        let hub = hub_with("srv");
        assert!(hub.leave_voice("srv", Uuid::new_v4()).is_none());
    }

    #[test]
    fn mute_reflete_na_presenca_e_diz_o_canal() {
        let hub = hub_with("srv");
        let canal = Uuid::new_v4();
        let eu = Uuid::new_v4();
        hub.join_voice("srv", canal, peer(eu), 6).unwrap();

        assert_eq!(hub.set_muted("srv", eu, true), Some(canal));
        assert!(hub.voice_presence("srv", canal)[0].muted);

        // Quem não está em canal nenhum não tem o que mutar.
        assert_eq!(hub.set_muted("srv", Uuid::new_v4(), true), None);
    }

    #[test]
    fn same_voice_channel_so_e_verdade_dentro_do_mesmo_canal() {
        let hub = hub_with("srv");
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let dentro1 = Uuid::new_v4();
        let dentro2 = Uuid::new_v4();
        let noutro = Uuid::new_v4();
        let fora = Uuid::new_v4();

        hub.join_voice("srv", a, peer(dentro1), 6).unwrap();
        hub.join_voice("srv", a, peer(dentro2), 6).unwrap();
        hub.join_voice("srv", b, peer(noutro), 6).unwrap();

        assert!(same_voice_channel(&hub, "srv", dentro1, dentro2));
        assert!(
            !same_voice_channel(&hub, "srv", dentro1, noutro),
            "offer não pode cruzar canal"
        );
        assert!(
            !same_voice_channel(&hub, "srv", dentro1, fora),
            "offer não pode ir pra quem não está em call"
        );
        assert!(
            !same_voice_channel(&hub, "srv", fora, fora),
            "quem não está em canal nenhum não fala com ninguém"
        );
    }

    #[test]
    fn servidores_sao_isolados_entre_si() {
        let hub = Hub::new();
        hub.get_or_create("srv-a");
        hub.get_or_create("srv-b");
        let canal = Uuid::new_v4();
        let eu = Uuid::new_v4();

        hub.join_voice("srv-a", canal, peer(eu), 6).unwrap();

        assert_eq!(hub.live_in("srv-a", canal), 1);
        assert_eq!(hub.live_in("srv-b", canal), 0);
        assert_eq!(hub.voice_channel_of("srv-b", eu), None);
        assert_eq!(hub.active_servers(), 2);
    }

    #[test]
    fn publish_em_servidor_sem_ninguem_e_no_op() {
        let hub = Hub::new();
        // Não pode criar entrada nem entrar em pânico só porque ninguém está lá.
        hub.publish("nunca-conectado", Uuid::new_v4(), ServerMsg::Pong);
        assert_eq!(hub.active_servers(), 0);
    }

    #[test]
    fn snapshot_lista_todos_os_canais_com_gente() {
        let hub = hub_with("srv");
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        hub.join_voice("srv", a, peer(Uuid::new_v4()), 6).unwrap();
        hub.join_voice("srv", b, peer(Uuid::new_v4()), 6).unwrap();

        let snap = hub.voice_snapshot("srv");
        assert_eq!(snap.len(), 2);
        assert_eq!(snap.iter().map(|c| c.peers.len()).sum::<usize>(), 2);
    }
}
