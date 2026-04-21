# BoraCall — handoff

Voice call desktop app — **Tauri v2 client + Rust backend + Postgres + WebRTC mesh**. Cross-platform (macOS/Windows/Linux). WebRTC peer-to-peer via a Rust signaling server; real mic, real peers, real audio.

---

## Visão geral da stack

```
┌────────────────┐        WebSocket /ws/rooms/{slug}        ┌──────────────────┐
│  BoraCall.app  │ ─── SDP/ICE/presença (JSON)  ────────── │  Rust server     │
│  (Tauri + JS)  │ ─── HTTPS /api/auth,/rooms ──────────── │  (axum 0.8)      │
│  WebRTC mesh   │                                          │  sqlx+postgres   │
└────────────────┘                                          │  jemalloc+tokio  │
        ▲                                                   └──────────────────┘
        │ P2P RTP/SRTP via ICE (áudio real)                         │
        └───────────── direto com outros peers ──┐                 ▼
                                                 ▼         ┌──────────────────┐
                                        ┌────────────────┐ │   Postgres 16    │
                                        │ Outro cliente  │ │   (Colima/dev    │
                                        │ BoraCall       │ │    ou VPS/prod)  │
                                        └────────────────┘ └──────────────────┘
```

- Server só faz **signaling** (troca de SDP/ICE + presença). **Não toca áudio** — mesh P2P até ~4 pessoas.
- Banco Postgres 16 via sqlx com pool sized for I/O (`4 × CPU`, clamp 8..64).
- Server é **stateless**: todo estado efêmero (presença por sala) está em um `DashMap` in-process. Pra escala horizontal, trocar pelo trait `SignalBus` (ver *Multi-nó* abaixo).

---

## Cross-platform em uma frase

O código (Rust + JS) é idêntico nas três plataformas. Tauri usa o webview nativo:
- **macOS** → WKWebView
- **Windows** → WebView2 (Edge/Chromium)
- **Linux** → WebKitGTK (`libwebkit2gtk-4.1-0`)

`npm run build` produz o bundle nativo da plataforma **onde você tá**:
- macOS: `.app` + `.dmg`
- Windows: `.msi` + `.exe` (NSIS)
- Linux: `.deb` + `.rpm` + `.AppImage`

Pra gerar os três ao mesmo tempo: GitHub Actions com três runners.

---

## Como rodar no dev

Pré-req: Rust, Node ≥20, **Colima** + Docker (pra Postgres local).

```bash
# 1) Sobe o banco
colima start --cpu 4 --memory 4 --disk 30     # uma vez só
docker-compose up -d                          # Postgres em 127.0.0.1:5432

# 2) Sobe o server Rust (terminal separado)
export DATABASE_URL=postgres://boracall:boracall@127.0.0.1:5432/boracall
export BC_JWT_SECRET="dev-only-insecure-secret-change-in-prod-please"
cargo run -p boracall-server --release

# 3) Abre o desktop
open src-tauri/target/release/bundle/macos/BoraCall.app
# OU o modo dev com hot reload do front:
npm run dev
```

O server sobe em `127.0.0.1:3030`. O desktop lê `window.BC_API_URL` em `dist/env.js` (default = localhost). Em prod, sobrescreva esse arquivo no build pra apontar pro domínio.

**Testar call com duas pessoas na mesma máquina:**
```
open -n src-tauri/target/release/bundle/macos/BoraCall.app
open -n src-tauri/target/release/bundle/macos/BoraCall.app
```
Cada instância é independente, cada uma faz signup com e-mail diferente, uma cria sala, copia o link (`boracall.app/s/<slug>` — substitui pelo slug real), outra cola em **"Entrar por link"**, e a voz flui entre as duas via WebRTC loopback.

---

## Estrutura do repo

```
BoraCall/
├── Cargo.toml                  # workspace (server + src-tauri)
├── docker-compose.yml          # postgres 16-alpine tunado pra dev
├── .env.example
├── package.json                # @tauri-apps/cli
├── HANDOFF.md                  # este arquivo
│
├── dist/                       # Frontend embebido no binário Tauri
│   ├── index.html
│   ├── env.js                  # window.BC_API_URL — sobrescreve no deploy
│   ├── api.js                  # REST client (window.api)
│   ├── realtime.js             # WebSocket client (window.Realtime)
│   ├── webrtc.js               # mesh manager (window.WebRTCMesh)
│   ├── desktop-bridge.js       # shims nativos (window.desktop)
│   ├── app.jsx                 # router + estado global + auth boot
│   ├── components.jsx          # Mark, Tweaks, UserRow, CopyButton...
│   ├── screens-1.jsx           # landing, auth, otp, onboarding, dashboard
│   ├── screens-2.jsx           # create, join, invite, precall, call, postcall, settings
│   ├── styles.css, screens.css
│   └── vendor/                 # React 18.3, Babel standalone, Inter+JetBrains Mono
│
├── server/                     # Rust backend
│   ├── Cargo.toml
│   ├── migrations/0001_initial.sql
│   └── src/
│       ├── main.rs             # bootstrap + routing + graceful shutdown
│       ├── config.rs           # env-driven config (BC_BIND, DATABASE_URL, BC_JWT_SECRET...)
│       ├── db.rs               # Pg pool (WAL-like tuning, pool sizing)
│       ├── state.rs            # AppState compartilhado
│       ├── auth.rs             # JWT + argon2id + AuthUser extractor
│       ├── error.rs            # AppError + IntoResponse
│       ├── signaling.rs        # Hub + WebSocket handler per-room
│       └── handlers/
│           ├── auth.rs         # signup, login, verify-otp, me, update_me
│           ├── rooms.rs        # list, create, get, join
│           └── system.rs       # health, version, stats
│
└── src-tauri/                  # Desktop shell nativo
    ├── Cargo.toml              # tauri v2 + plugin-clipboard-manager + plugin-opener
    ├── tauri.conf.json         # janela 1100×720, bundle cross-platform
    ├── Info.plist              # NSMicrophoneUsageDescription + scheme boracall://
    ├── capabilities/default.json
    ├── icons/*                 # set completo (macOS/Windows/Linux/iOS/Android)
    └── src/
        ├── main.rs, lib.rs     # platform_info, window_*, set_invisible_mode
        └── build.rs
```

---

## Superfície da API

Base: `http://127.0.0.1:3030` (dev). Todas as rotas autenticadas exigem `Authorization: Bearer <jwt>`.

| Método | Rota                              | Auth | Descrição                                           |
|--------|-----------------------------------|------|-----------------------------------------------------|
| GET    | `/api/health`                     | —    | `ok`                                                |
| GET    | `/api/version`                    | —    | `{ name, version }`                                 |
| GET    | `/api/stats`                      | —    | `{ active_rooms }`                                  |
| POST   | `/api/auth/signup`                | —    | `{ email, password, display_name? }` → `{ token, user }` |
| POST   | `/api/auth/login`                 | —    | `{ email, password }` → `{ token, user }`           |
| POST   | `/api/auth/verify-otp`            | ✓    | `{ code }` → `{ user }` (dev stub — qualquer código vale) |
| GET    | `/api/auth/me`                    | ✓    | → `{ id, email, display_name, email_verified }`     |
| PATCH  | `/api/auth/me`                    | ✓    | `{ display_name? }` → `user`                        |
| GET    | `/api/rooms`                      | ✓    | `[{room...}]` (suas + onde é membro)                |
| POST   | `/api/rooms`                      | ✓    | `{ name, room_type, password? }` → `room`           |
| GET    | `/api/rooms/{slug}`               | ✓    | → `room`                                            |
| POST   | `/api/rooms/{slug}/join`          | ✓    | `{ password? }` → `room`                            |
| GET    | `/ws/rooms/{slug}?token=<jwt>`    | ✓ (query) | WebSocket signaling                             |

### Protocolo WebSocket (JSON por frame de texto)

**Cliente → servidor** (`ClientMsg`):

```json
{"type":"offer",    "to":"<uuid>", "sdp":"..."}
{"type":"answer",   "to":"<uuid>", "sdp":"..."}
{"type":"ice",      "to":"<uuid>", "candidate":{...}}
{"type":"mute",     "muted":true}
{"type":"speaking", "level":0.42}
{"type":"leave"}
{"type":"ping"}
```

**Servidor → cliente** (`ServerMsg`):

```json
{"type":"presence", "peers":[{"user_id":"...","display_name":"...","muted":false}]}
{"type":"joined",   "peer":{...}}
{"type":"left",     "user_id":"..."}
{"type":"offer",    "from":"<uuid>","sdp":"..."}
{"type":"answer",   "from":"<uuid>","sdp":"..."}
{"type":"ice",      "from":"<uuid>","candidate":{...}}
{"type":"mute",     "user_id":"...","muted":true}
{"type":"speaking", "user_id":"...","level":0.4}
{"type":"error",    "message":"..."}
{"type":"pong"}
```

Handshake: upgrade direto; sem membership pré-existente o server faz *auto-join* (idempotente).  
Glare avoidance: o peer com `user_id` lexicográfico MENOR cria o offer.

---

## Decisões de arquitetura

### Por que signaling-first, sem SFU
Voz em grupo até 4 pessoas fecha mesh bem: 6 conexões P2P, latência ótima, zero custo de server pra áudio. Acima disso (5+), o overhead cresce quadraticamente e vale subir um SFU. **LiveKit** (Go) ou **mediasoup** (Node) são as escolhas maduras — rodam em docker ao lado do BoraCall quando a hora chegar.

### Por que Postgres 16
Prod-grade, tipos ricos (uuid, jsonb, citext), extensões úteis, mainstream em VPS e em hosted (Neon, Supabase, RDS). sqlx checa queries em compile-time contra o DB vivo — custo baixo quando o Postgres tá no Colima.

### Por que mesh single-node hoje
Ler o código: `signaling.rs` tem um `Hub` com `DashMap<slug, RoomChannel>`. Cada sala é um `tokio::sync::broadcast`. Simples, rápido, e cabe em um único nó facilmente milhares de salas pequenas. Um ARM VPS de 4 vCPU aguenta ~10k conexões WebSocket ociosas + signaling burst.

### Production allocator
`tikv-jemallocator` como `#[global_allocator]`. Malloc default do glibc vaza memória sob carga WebSocket de longa duração; jemalloc é estável.

### Password hashing
Argon2id (o OWASP "primary") com salt aleatório por usuário. Custos default do crate argon2 0.5 (`m_cost=19456 KiB, t_cost=2, p=1`) — balanceados pra server moderno.

### JWT stateless
HS256. `BC_JWT_SECRET` precisa ter ≥24 chars. TTL default: 30 dias (config `BC_JWT_TTL_DAYS`). Token guardado no `localStorage` do webview — em prod com mais paranoia, migrar pra cookie `HttpOnly + SameSite=Strict` + endpoint `POST /auth/refresh`.

---

## Multi-nó / escala horizontal

Quando precisar rodar 2+ instâncias do server atrás de um LB:

1. **Sticky sessions no LB** por `ip_hash` (nginx) ou cookie-based (HAProxy). Mantém a conexão WebSocket num nó só.
2. **Bus pub/sub entre nós** — adicionar um trait `SignalBus` em `signaling.rs` com 2 impls:
   - `LocalBus` (hoje, tokio::broadcast)
   - `NatsBus` — usando [`async-nats`](https://crates.io/crates/async-nats). Leve, ~1MB de binário extra. JetStream opcional pra replay em reconexão.
3. **Presence global** — idem via NATS KV ou Redis Hash com TTL curto (3s heartbeat).
4. **Postgres → read replica** quando o gargalo virar leitura (auth/room lookup).

**Estimativa**: até ~5k usuários simultâneos em single-node 4 vCPU fica confortável. Acima disso, vale horizontal.

---

## Deploy em VPS (prod)

```bash
# Na VPS (Ubuntu 22.04+)

# 1) Instalar dependências do build
apt install -y build-essential curl libssl-dev postgresql-client
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2) Clonar + build
git clone <repo>
cd BoraCall
cargo build -p boracall-server --release
sudo install -o boracall -m 755 target/release/boracall-server /usr/local/bin/

# 3) Rodar Postgres gerenciado (Neon, Supabase, RDS) OU instalar local:
apt install -y postgresql-16
sudo -u postgres createuser boracall
sudo -u postgres createdb -O boracall boracall
sudo -u postgres psql -c "ALTER USER boracall WITH PASSWORD '<senha-strong>';"

# 4) systemd unit (/etc/systemd/system/boracall-server.service):
[Unit]
Description=BoraCall signaling server
After=network.target postgresql.service

[Service]
Type=simple
User=boracall
Environment=BC_BIND=127.0.0.1:3030
Environment=DATABASE_URL=postgres://boracall:***@127.0.0.1:5432/boracall
Environment=BC_JWT_SECRET=<long-random>
Environment=BC_CORS_ANY=0
Environment=RUST_LOG=info
ExecStart=/usr/local/bin/boracall-server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target

systemctl enable --now boracall-server

# 5) nginx reverse proxy com TLS (Certbot/Let's Encrypt):
server {
    listen 443 ssl http2;
    server_name api.boracall.app;
    ssl_certificate     /etc/letsencrypt/live/api.boracall.app/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/api.boracall.app/privkey.pem;

    # WebSocket precisa Upgrade
    location /ws/ {
        proxy_pass http://127.0.0.1:3030;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
    location /api/ {
        proxy_pass http://127.0.0.1:3030;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

Depois, no build do desktop, edita `dist/env.js` antes do `npm run build`:
```js
window.BC_API_URL = "https://api.boracall.app";
```

---

## Segurança — o que falta antes de ir pra produção

- [ ] TURN server (coturn) — **~30% dos users em simétrica NAT não conectam sem ele**. Subir coturn no mesmo VPS, adicionar credenciais aos `iceServers` do `webrtc.js`.
- [ ] OTP real (Resend/Postmark/SES) em vez do stub.
- [ ] Rate limit por IP — `tower-http` tem layer pronto, falta plugar.
- [ ] Rotação de JWT + refresh tokens (hoje TTL é 30 dias fixo).
- [ ] Password reset endpoint.
- [ ] CSRF — não aplica agora (API stateless via Bearer token), mas avaliar se migrar pra cookie.
- [ ] Logs estruturados sanitizados (hoje tracing simples com env-filter).
- [ ] Backup Postgres (pg_basebackup ou wal-g via systemd timer).
- [ ] Code-signing:
  - macOS: Apple Developer ID ($99/ano) + notarização
  - Windows: certificado EV (~$300/ano)
  - Linux: sign com GPG pra repositórios .deb/.rpm

---

## `window.desktop` API (shell nativo)

```js
window.desktop.isNative          // true em Tauri, false em browser
window.desktop.platform          // "macos" / "windows" / "linux" / null
window.desktop.arch, version, appVersion

window.desktop.clipboard.writeText(text) / .readText()
window.desktop.window.minimize() / .toggleMaximize() / .setInvisibleMode(bool)
window.desktop.opener.openUrl(url)
window.desktop.invoke("platform_info")      // ou qualquer comando Rust registrado
```

---

## `window.api` (REST client)

```js
await window.api.signup({ email, password, displayName })
await window.api.login({ email, password })
await window.api.verifyOtp("123456")
await window.api.me()
await window.api.updateMe({ displayName })
await window.api.listRooms()
await window.api.createRoom({ name, type: "ephemeral", pw: null })
await window.api.getRoom(slug)
await window.api.joinRoom(slug, password)
window.api.logout()
```

Retornos vêm normalizados — rooms têm `{id, slug, name, type, live(bool), count, locked, lastActive(human), members:[]}`.

---

## `window.Realtime` (WebSocket) + `window.WebRTCMesh`

```js
// 1) Conectar signaling
const rt = new Realtime(slug);           // usa api.getToken()
rt.on("presence", m => {...});
rt.on("joined",   m => {...});
rt.on("offer",    m => {...});
rt.connect();

// 2) Montar a mesh em cima
const mesh = new WebRTCMesh.Mesh(rt, selfUserId);
mesh.on("peers", peers => renderPeers(peers));
mesh.on("local-level", lvl => showMyMeter(lvl));
await mesh.start();                      // acquireMic + addTrack + createOffer per peer

mesh.setMuted(true);
mesh.stop();  rt.close();
```

---

## O que é real vs stub

**Real:**
- Auth (signup/login/me/update) — Postgres + argon2id + JWT
- Rooms CRUD — Postgres
- Signaling WebSocket — Rust (axum + tokio broadcast)
- WebRTC mesh — navigator.mediaDevices + RTCPeerConnection, até ~4 peers
- Presence (joined/left/muted/speaking) em tempo real
- Cross-platform build matrix
- Clipboard/Window/Opener nativos (Tauri)
- Fontes vendoradas — zero rede no boot

**Stub / falta antes de prod:**
- OTP é dev stub (qualquer código valida)
- Sem TURN server (peers atrás de NAT simétrica não conectam)
- Sem SFU (limite prático: 4 pessoas simultâneas)
- Deep link `boracall://` registrado mas sem handler Rust
- Google login — botão existe, não liga em OAuth
- Sem rate limit nos endpoints
- Sem auto-update

---

## Problemas conhecidos

- **Gatekeeper macOS** na primeira execução (app não assinado). `xattr -c BoraCall.app` ou clique direito → Abrir.
- **Cache do WebView** entre reloads: se mudar JSX e não ver, feche e abra o app (ou use `npm run dev`).
- **Permissão de microfone**: o macOS só pergunta quando `getUserMedia` é chamado. Se negar uma vez, precisa liberar em *Ajustes do Sistema → Privacidade → Microfone*. Info.plist tem descrição em PT-BR.
- **WebRTC em WKWebView** funciona mas tem uma quirk: alguns codecs avançados não estão disponíveis. Opus estéreo 48kHz (o que usamos) roda de boa.

---

## Comandos úteis

```bash
# Backend
cargo run -p boracall-server --release      # subir local
cargo check -p boracall-server               # type-check rápido
docker-compose up -d postgres                # só o banco
docker-compose down -v                       # derruba + apaga dados

# Frontend (Tauri)
npm run dev                                  # hot reload
npm run build                                # bundle nativo da plataforma atual
npx tauri icon <arquivo>                     # regenera ícones

# Abrir N instâncias pro teste de call (macOS)
for i in 1 2; do open -n src-tauri/target/release/bundle/macos/BoraCall.app; done
```
