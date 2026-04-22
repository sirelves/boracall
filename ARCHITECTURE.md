# Arquitetura do BoraCall

Documento voltado a quem vai **ler o código** ou **contribuir**. Para um overview
de portfolio veja [README.md](./README.md); para operação em produção
(VPS, systemd, nginx, TURN, backup, code-signing) veja [HANDOFF.md](./HANDOFF.md).

---

## Princípios

1. **Server stateless ao máximo** — persistência é do Postgres; estado efêmero
   (presença por sala) é in-process num `DashMap` e pode ser trocado por um bus
   pub/sub sem mudar a superfície HTTP/WS.
2. **Servidor não toca áudio** — WebRTC puro ponto-a-ponto (mesh). O server é
   apenas **signaling + presence + auth**. Zero RTP/SRTP no caminho.
3. **Zero build step no frontend** — `dist/` é copiado direto pro bundle Tauri.
   React + Babel standalone via `<script>`. Nenhum webpack, nenhum Vite.
4. **Cross-platform é do webview nativo, não do bundle** — mesmo código JS + Rust
   nas 3 plataformas; o shell troca o webview (WKWebView / WebView2 / WebKitGTK).

---

## Visão geral (ASCII)

```
 ┌──────────────────────────────────────────────────────────────────────┐
 │                        BoraCall.app  (Tauri v2)                      │
 │                                                                      │
 │  ┌────────────────── webview nativo ──────────────────┐              │
 │  │  index.html                                        │              │
 │  │  ├─ window.api            (REST client)            │              │
 │  │  ├─ window.Realtime       (WebSocket reconnect)    │              │
 │  │  ├─ window.WebRTCMesh     (RTCPeerConnection/peer) │              │
 │  │  └─ window.desktop        (bridge nativa)          │              │
 │  └─────────────────────────┬──────────────────────────┘              │
 │                            │                                         │
 │  ┌─────── Rust (src-tauri) ▼────── (tauri::command) ─────┐          │
 │  │  platform_info, window_*, set_invisible_mode,         │          │
 │  │  check_for_update, install_update                     │          │
 │  │  + webkit2gtk permission hook (Linux)                 │          │
 │  └───────────────────────────────────────────────────────┘          │
 └──────────────────────────────────────────────────────────────────────┘
                  │                                  ▲
                  │ HTTPS /api/*                     │
                  │ WSS   /ws/rooms/:slug            │ RTP/SRTP P2P
                  ▼                                  │ (direto com peers)
 ┌────────────────────────────── boracall-server ──────────────┐       │
 │  axum 0.8  +  tokio  +  jemalloc                            │       │
 │                                                             │       │
 │  ┌────────────────┐   ┌───────────────────────────────────┐ │       │
 │  │  handlers/     │   │  signaling::Hub                   │ │       │
 │  │    auth.rs     │   │  DashMap<slug, RoomChannel>       │ │       │
 │  │    rooms.rs    │   │    RoomChannel {                  │ │       │
 │  │    system.rs   │   │      tx: broadcast<Envelope>,     │ │       │
 │  └──────┬─────────┘   │      peers: RwLock<Vec<Peer>>,    │ │       │
 │         │             │    }                              │ │       │
 │         ▼             └────────────┬──────────────────────┘ │       │
 │  ┌────────────────┐                │                        │       │
 │  │   sqlx pool    │                │ per-conn tokio task    │       │
 │  │ (4×CPU, 8..64) │                │ (split + select!)      │       │
 │  └──────┬─────────┘                ▼                        │       │
 └─────────┼───────────────────────────────────────────────────┘       │
           ▼                                                           │
 ┌─────────────────────────────┐                                       │
 │   Postgres 16               │     ┌──────────────────┐              │
 │   users, rooms, memberships │     │   Outro peer     │──────────────┘
 │   call_events               │     │   BoraCall.app   │   mesh P2P
 └─────────────────────────────┘     └──────────────────┘
```

---

## Fluxo 1 — Signup com OTP por e-mail

```
Cliente                       boracall-server                  Postgres         Resend
   │                                 │                            │               │
   │  POST /api/auth/signup          │                            │               │
   │  { email, password, name? }     │                            │               │
   │────────────────────────────────>│                            │               │
   │                                 │  argon2id(password)        │               │
   │                                 │  INSERT INTO users  ─────> │               │
   │                                 │                            │               │
   │                                 │  UNIQUE violation? ──> 409 Conflict        │
   │                                 │                            │               │
   │                                 │  JWT.sign(sub=user_id)     │               │
   │<─────── 200 {token, user} ──────│                            │               │
   │  localStorage.bc_token = token  │                            │               │
   │                                 │                            │               │
   │  POST /api/auth/request-otp     │                            │               │
   │────────────────────────────────>│                            │               │
   │                                 │  OtpStore.issue(email,     │               │
   │                                 │     ttl=10min)  (in-mem)   │               │
   │                                 │  Mailer.send ──────────────────────────────>│
   │<─────── 204 No Content ─────────│                            │               │
   │                                 │                            │               │
   │  POST /api/auth/verify-otp      │                            │               │
   │  { code }                       │                            │               │
   │────────────────────────────────>│                            │               │
   │                                 │  OtpStore.verify(email,    │               │
   │                                 │     code) → ok             │               │
   │                                 │  UPDATE users SET          │               │
   │                                 │    email_verified=true ──> │               │
   │<─────── 200 {user} ─────────────│                            │               │
```

**Notas:**
- `OtpStore` é um `DashMap<email, (code_hash, expires_at)>` — perde em restart, que é aceitável pra TTL de 10min.
- `Mailer` usa `resend-rs` com API key por env. Se `BC_RESEND_API_KEY` não estiver setado, o server loga o OTP em `info!` (dev mode).
- Password reset segue o mesmo padrão — `request-password-reset` → `reset-password` com `{email, code, new_password}`.

---

## Fluxo 2 — Criar sala e convidar

```
Host                              boracall-server                     Postgres
  │                                     │                                  │
  │  POST /api/rooms                    │                                  │
  │  { name, room_type, password? }     │                                  │
  │────────────────────────────────────>│                                  │
  │  Authorization: Bearer <jwt>        │                                  │
  │                                     │  slug = random_slug()  (5 chars) │
  │                                     │  (retry on UNIQUE conflict)      │
  │                                     │  argon2id(password) if set       │
  │                                     │  INSERT INTO rooms ───────────── >│
  │                                     │  INSERT INTO memberships (host)─>│
  │<─────── 200 {room} ─────────────────│                                  │
  │                                     │                                  │
  │  clipboard.writeText(               │                                  │
  │    "https://boracall.app/s/" + slug)│                                  │
  │                                     │                                  │
  │  ─── (envia link fora da app) ───  │                                  │
```

Slug curto (5 chars, alfabeto sem `0/O/1/l/I`) = fácil de falar por voz, difícil
de enumerar sem um UNIQUE hit — e o INSERT faz retry em conflict.

---

## Fluxo 3 — Segundo peer entra e começa a falar

Este é o caso central. Mostra signaling + handshake + mídia.

```
Peer B (novo)                       Server                           Peer A (já na sala)
   │                                   │                                     │
   │  GET /ws/rooms/:slug              │                                     │
   │  Sec-WebSocket-Protocol:          │                                     │
   │    bc.v1, token.<jwt>             │                                     │
   │──────────────────────────────────>│                                     │
   │                                   │  decode_token(jwt) → user_id        │
   │                                   │  SELECT rooms WHERE slug=?          │
   │                                   │  if locked → check memberships      │
   │                                   │     else   → INSERT membership      │
   │                                   │  check hub.slug_count < max_peers   │
   │<─ 101 Switching Protocols ────────│  reply with protocol: bc.v1         │
   │   (bc.v1 accepted)                │                                     │
   │                                   │                                     │
   │                                   │  hub.add_peer(slug, B)              │
   │<─── {"type":"presence",           │  broadcast.send({type:"joined"})    │
   │       peers:[A]} ─────────────────│──────────────────────> (A recebe)   │
   │                                   │                                     │
   │  (glare avoidance:                │                                     │
   │   B.user_id < A.user_id? Não.     │                                     │
   │   B.user_id > A.user_id? Sim.     │                                     │
   │   → A inicia o offer.)            │                                     │
   │                                   │                                     │
   │                                   │            ◄── pc.createOffer()    │
   │                                   │            {"type":"offer",        │
   │<── {"type":"offer",from:A,sdp} ──│◄── to:B,sdp ────────────────────────│
   │                                   │                                     │
   │  pc.setRemoteDescription(offer)   │                                     │
   │  pc.createAnswer()                │                                     │
   │  pc.setLocalDescription(answer)   │                                     │
   │── {"type":"answer",to:A,sdp} ───>│──── {"type":"answer",from:B,sdp}──>│
   │                                   │                                     │
   │                                   │                   ◄── ICE candidates│
   │── ICE candidates ────────────────>│──── ICE candidates ────────────────>│
   │                                   │                                     │
   │                                   │       (ICE trickle em ambos)        │
   │                                   │                                     │
   │══════════════════ RTP/SRTP P2P (áudio Opus 48kHz) ═════════════════════│
   │                                                                         │
   │  attachLevelMeter() → dispara    "speaking" via WS quando > 0.06 RMS   │
   │                                                                         │
   │── {"type":"speaking",level:0.4} ─>│─── broadcast ───── {"type":"speaking",
   │                                   │                      user_id,level}─>│
```

**Detalhes importantes:**

- **JWT via subprotocol**, não via `?token=...`. `extract_token()` varre
  `Sec-WebSocket-Protocol`, o server ecoa apenas `bc.v1` de volta.
- **Glare avoidance** é puramente determinístico: `String(self) < String(other) → self cria offer`.
  Nenhum round-trip extra pra negociar quem inicia.
- **Duas tasks por conexão** (`forward` e `ingest`), juntadas por `tokio::select!`.
  Quando qualquer uma cai, a outra é abortada e o peer é removido do hub.
- **Presence é autoritativa do server**: snapshot completo no join, `joined`/`left`
  em tempo real, `mute`/`speaking` merged dentro do `Peer` struct.
- **Filtro de echo**: `if env.origin == user_id && env.target != Some(user_id)` —
  você nunca recebe a própria mensagem a menos que o server tenha endereçado ela a você
  (usado pro `Pong`).

---

## Mesh topology

Pra N peers, o mesh tem `N*(N-1)/2` conexões:

| N peers | conexões | uplink de cada peer |
|---------|----------|---------------------|
| 2       | 1        | 1× Opus ~32 kbps    |
| 3       | 3        | 2× = 64 kbps        |
| 4       | 6        | 3× = 96 kbps        |
| 5       | 10       | 4× = 128 kbps       |
| 6       | 15       | 5× = 160 kbps       |

O gargalo não é o **server** (só metadata) nem a **CPU do cliente** (Opus decode é
barato); é o **uplink residencial típico** (~1–3 Mbps). Com múltiplos peers
**também transmitindo vídeo** seria bem pior — por isso BoraCall é só áudio e
fica capado em 6 via `BC_MAX_PEERS_PER_ROOM` (default).

Quando fizer sentido SFU (LiveKit / mediasoup), o fluxo muda pra:

```
Peer ──audio──> SFU ──N-1 downstreams──> Peers
```

O server BoraCall fica como **control plane** (auth + room metadata) e o SFU é
adicionado como daemon separado. A superfície WS atual não precisa mudar — basta
um flag de feature `{"use_sfu": true}` no `presence` que diz aos clientes pra
conectarem no SFU em vez de abrirem `RTCPeerConnection`s diretos.

---

## Protocolo WebSocket (JSON por frame de texto)

### Cliente → servidor (`ClientMsg`)

```jsonc
{"type": "offer",    "to": "<uuid>", "sdp": "..."}
{"type": "answer",   "to": "<uuid>", "sdp": "..."}
{"type": "ice",      "to": "<uuid>", "candidate": {...}}
{"type": "mute",     "muted": true}
{"type": "speaking", "level": 0.42}       // 0..1, coalescido no cliente
{"type": "leave"}
{"type": "ping"}
```

### Servidor → cliente (`ServerMsg`)

```jsonc
{"type": "presence", "peers": [{"user_id":"...","display_name":"...","muted":false}]}
{"type": "joined",   "peer": {...}}
{"type": "left",     "user_id": "..."}
{"type": "offer",    "from": "<uuid>", "sdp": "..."}
{"type": "answer",   "from": "<uuid>", "sdp": "..."}
{"type": "ice",      "from": "<uuid>", "candidate": {...}}
{"type": "mute",     "user_id": "...", "muted": true}
{"type": "speaking", "user_id": "...", "level": 0.4}
{"type": "error",    "message": "..."}
{"type": "pong"}
```

Handshake: upgrade direto via `Sec-WebSocket-Protocol: bc.v1, token.<jwt>`.
Salas **unlocked** fazem auto-join idempotente (UX dum clique).
Salas **locked** exigem POST prévio em `/api/rooms/:slug/join` com senha —
caso contrário o upgrade responde `403`.

---

## Modelo de dados

```
                       ┌────────────────────┐
                       │       users        │
                       │────────────────────│
                       │  id  uuid PK       │
                       │  email  citext UQ  │
                       │  password_hash     │
                       │  display_name      │
                       │  email_verified    │
                       │  created_at        │
                       │  updated_at  (trg) │
                       └─────────┬──────────┘
                                 │
                     ┌───────────┴────────────┐
                     │                        │
                     ▼                        ▼
           ┌──────────────────┐     ┌──────────────────────┐
           │     rooms        │     │     memberships      │
           │──────────────────│     │──────────────────────│
           │ id uuid PK       │<─┐  │ room_id  FK(rooms)   │
           │ slug text UQ     │  │  │ user_id  FK(users)   │
           │ name             │  └──│ role  host|member    │
           │ room_type        │     │ joined_at            │
           │   ephemeral/     │     │ PK (room_id,user_id) │
           │   persistent     │     └──────────────────────┘
           │ password_hash ?  │
           │ created_by FK    │               ┌──────────────────────┐
           │ created_at       │               │     call_events      │
           │ last_active_at   │◄──────────────│──────────────────────│
           └──────────────────┘               │ id uuid PK           │
                                              │ room_id  FK          │
                                              │ user_id  FK          │
                                              │ kind text            │
                                              │ payload jsonb        │
                                              │ occurred_at          │
                                              └──────────────────────┘
```

- `users.email` é **CITEXT** (case-insensitive unique sem `LOWER(...)` manual).
- `rooms.slug` é lexical (TEXT) — curto, mas independente do UUID (não vaza id).
- `memberships` é composite PK — uma linha por par.
- `call_events` é **append-only**: serve como log de auditoria e base para métricas
  de pós-chamada (quem entrou quando, muted time, etc).

Trigger `users_set_updated_at` faz touch automático no `updated_at` em qualquer UPDATE.

---

## Escala horizontal

Single-node vai longe (um VPS 4 vCPU aguenta ~5k usuários simultâneos em
signaling). Quando precisar de 2+ nós atrás de LB:

1. **Sticky sessions por IP hash** no LB (nginx `ip_hash`, HAProxy cookie) — mantém
   uma conexão WS num nó só.
2. **Trait `SignalBus`** em `signaling.rs` com duas impls:
   ```rust
   trait SignalBus {
       async fn publish(&self, slug: &str, env: Envelope);
       async fn subscribe(&self, slug: &str) -> broadcast::Receiver<Envelope>;
   }
   ```
   Hoje só existe `LocalBus` (DashMap + tokio::broadcast). Adicionar
   `NatsBus` com [`async-nats`](https://crates.io/crates/async-nats) — ~1MB de binário
   extra. JetStream opcional pra replay em reconexão.
3. **Presence global** idem via NATS KV ou Redis Hash com TTL de 3s + heartbeat.
4. **Postgres read replica** quando leitura (auth/room lookup) virar gargalo.

O resto do código **não muda** — a superfície HTTP/WS é idêntica.

---

## Decisões de engenharia justificadas

### Por que Rust no backend
- Memory safety + zero-cost abstractions importam em processo long-running com
  milhares de WebSockets abertas.
- sqlx valida SQL **em compile-time** contra um Postgres vivo.
- O binário final é um executável estático — deploy é `scp + systemd`, sem runtime.

### Por que Postgres 16 (e não SQLite / MongoDB)
- Tipos ricos (`uuid`, `citext`, `jsonb`) fazem schema expressivo sem ORM magic.
- Extensões (`pgcrypto`, `uuid-ossp`) eliminam código de UUID gen no app.
- Mainstream em VPS e managed (Neon, Supabase, RDS) — portabilidade de operação.

### Por que mesh e não SFU desde o dia 1
- Voz até 4 pessoas fecha mesh com latência ótima e zero custo de server de mídia.
- SFU adiciona operação não-trivial (coturn, codecs, recording, TURN).
- `BC_MAX_PEERS_PER_ROOM` é o guarda-corpo; SFU entra quando o produto justificar.

### Por que Tauri (e não Electron)
- Binário final ~5–10MB vs ~100MB do Electron.
- Webview nativo → lookup e performance alinhados com o sistema.
- Rust-first: `tauri::command` é tão idiomático quanto uma função normal.

### Por que React + Babel standalone (e não Vite / Next.js)
- Projeto tem ~2500 linhas de frontend — complexidade não justifica toolchain.
- Zero build step → edita JSX, reload, pronto. HMR pelo webview (dev) ou reload (prod).
- Fontes vendoradas + React CDN = zero requisição externa no boot do app.

### Por que JWT via subprotocol WS
- Query string (`?token=...`) vaza em **todos** estes lugares:
  - Access log do nginx
  - Histórico do browser
  - Referer headers
  - Métricas (Datadog, etc) que capturam URLs
- Subprotocol header não aparece em nenhum deles. Custo: 3 linhas de código.

### Por que jemalloc em prod
- Reproduzível: glibc malloc acumula fragmentação em processos com muitos
  `tokio::spawn` de vida curta. Jemalloc estabiliza RSS a longo prazo.
- Sem custo em dev (só ativa em `not(target_env = "msvc")`).

---

## Observabilidade

Hoje: `tracing` + `tracing-subscriber` com env-filter (`RUST_LOG=...`).
Deploy em prod loga em stdout → journald (systemd) → `journalctl -u boracall-server`.

Próximos passos (ver [ROADMAP](./README.md#roadmap)):
- `tracing-opentelemetry` exportando pra Tempo/Jaeger
- Métricas Prometheus (`metrics` crate + `/metrics` endpoint) pra
  - `bc_active_rooms`
  - `bc_active_peers_total`
  - `bc_ws_messages_total{direction,type}`
  - `bc_auth_attempts_total{kind,result}`

---

## Referências e leitura complementar

- [WebRTC Glossary](https://webrtcglossary.com/) — termos de mesh, SFU, TURN, trickle ICE.
- [OWASP Password Storage](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html) — Argon2id parameters.
- [RFC 6455](https://www.rfc-editor.org/rfc/rfc6455) + [RFC 6455 §4.2.2 — Sec-WebSocket-Protocol](https://www.rfc-editor.org/rfc/rfc6455#section-4.2.2).
- [Tauri v2 Docs](https://v2.tauri.app/) — commands, plugins, updater signing.
- [sqlx `query!` macro](https://docs.rs/sqlx/latest/sqlx/macro.query.html) — compile-time SQL checks.
