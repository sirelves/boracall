# Arquitetura do BoraCall

Documento voltado a quem vai **ler o código** ou **contribuir**. Para um overview
de portfolio veja [README.md](./README.md); para operação em produção
(VPS, systemd, nginx, TURN, backup, code-signing) veja ARCHITECTURE.md.

---

## Princípios

1. **Server stateless ao máximo** — persistência é do Postgres; estado efêmero
   (presença de voz por canal) é in-process num `DashMap` e pode ser trocado por um bus
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
 │  ┌─────── Rust (src-tauri) ▼────── (tauri::command) ─────┐           │
 │  │  platform_info, window_*, set_invisible_mode,         │           │
 │  │  check_for_update, install_update                     │           │
 │  │  + webkit2gtk permission hook (Linux)                 │           │
 │  └───────────────────────────────────────────────────────┘          │
 └──────────────────────────────────────────────────────────────────────┘
          │                        │                         ▲
          │ HTTPS /api/*           │ WSS /ws/servers/:slug   │ RTP/SRTP
          ▼                        ▼                         │ P2P
 ┌────────────────────────────── boracall-server ──────────────┐       │
 │  axum 0.8  +  tokio  +  jemalloc                            │       │
 │                                                             │       │
 │  ┌────────────────┐   ┌───────────────────────────────────┐ │       │
 │  │  handlers/     │   │  signaling::Hub                   │ │       │
 │  │    auth.rs     │   │  DashMap<server_slug, …>          │ │       │
 │  │    servers.rs  │   │    tx: broadcast<Envelope>        │ │       │
 │  │    messages.rs │   │    voice: RwLock<               > │ │       │
 │  │    ice.rs      │   │      HashMap<channel_id, Vec<Peer>>│ │      │
 │  │    system.rs   │   │                                   │ │       │
 │  └──────┬─────────┘   └────────────┬──────────────────────┘ │       │
 │         │  ratelimit (IP e e-mail) │ uma task por conexão   │       │
 │         ▼                          ▼ (split + select!)      │       │
 │  ┌────────────────┐                                         │       │
 │  │   sqlx pool    │                                         │       │
 │  │ (4×CPU, 8..64) │                                         │       │
 │  └──────┬─────────┘                                         │       │
 └─────────┼───────────────────────────────────────────────────┘       │
           ▼                                                           │
 ┌──────────────────────────────┐                                      │
 │   Postgres 16                │    ┌──────────────────┐              │
 │   users, servers, channels   │    │   Outro peer     │──────────────┘
 │   server_members, messages   │    │   BoraCall.app   │   mesh P2P
 │   message_reads, otp_codes   │    └──────────────────┘
 └──────────────────────────────┘              ▲
                                               │ quando o P2P direto
 ┌──────────────────────────────┐              │ não fecha (NAT simétrica)
 │   coturn (TURN)              │──────────────┘
 │   credencial efêmera         │
 │   via GET /api/ice           │
 └──────────────────────────────┘
```

**Uma conexão WebSocket por servidor, não por canal.** O usuário precisa, ao
mesmo tempo, receber mensagem de texto de todos os canais que enxerga e ver quem
está em cada canal de voz — enquanto fica dentro de no máximo um canal de voz.
Uma conexão por canal seria N conexões por usuário.

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

## Fluxo 2 — Criar servidor, canal e convidar

```
Dono                              boracall-server                     Postgres
  │                                     │                                  │
  │  POST /api/servers { name }         │                                  │
  │────────────────────────────────────>│                                  │
  │  Authorization: Bearer <jwt>        │                                  │
  │                                     │  BEGIN ─────────────────────────>│
  │                                     │  slug = random_slug()  (5 chars) │
  │                                     │  INSERT servers ────────────────>│
  │                                     │  INSERT server_members (owner) ─>│
  │                                     │  INSERT channels  # geral (text)>│
  │                                     │  INSERT channels  🔊 Geral(voice)>│
  │                                     │  COMMIT ────────────────────────>│
  │<──── 200 { server, channels[] } ────│                                  │
  │                                     │                                  │
  │  clipboard.writeText(               │                                  │
  │    "https://boracall.com/c/"+slug)  │   ← slug DO CANAL, não do servidor
  │                                     │                                  │
  │  ─── (envia link fora do app) ───   │                                  │
```

Três decisões que valem explicação:

**Criar servidor é transacional.** Nasce com dono e um canal de cada tipo, ou não
nasce. Servidor sem canal nenhum é um estado que o front não sabe renderizar. O
retry de colisão de slug recomeça a transação inteira, porque no Postgres um erro
aborta a transação.

**O slug de canal é global, não por servidor.** É ele que vira o link
compartilhável: `/c/<slug>` precisa resolver sozinho, sem exigir o slug do
servidor junto. É o que preserva o convite de baixo atrito.

**Slug de 5 chars, alfabeto sem `0/O/1/l/I`** — fácil de ditar por voz, difícil de
enumerar, e o INSERT faz retry em conflito de UNIQUE.

Quem recebe o link chama `GET /api/channels/<slug>`, que devolve o canal, o
servidor dono e se quem pediu **já é membro** — é isso que decide entre "entrar
direto" e "aceitar convite".

---

## Fluxo 3 — Segundo peer entra e começa a falar

Este é o caso central. Mostra signaling + handshake + mídia.

```
Peer B (novo)                       Server                       Peer A (já no canal)
   │                                   │                                     │
   │  GET /ws/servers/:slug            │                                     │
   │  Sec-WebSocket-Protocol:          │                                     │
   │    bc.v1, token.<jwt>             │                                     │
   │──────────────────────────────────>│                                     │
   │                                   │  decode_token(jwt) → user_id        │
   │                                   │  é membro do servidor? senão 404     │
   │<─ 101 Switching Protocols ────────│                                     │
   │   {"type":"voice_state", …}        │  snapshot de quem está em cada canal│
   │                                   │                                     │
   │  {"type":"join_voice",             │  canal existe, é deste servidor      │
   │    channel_id}                    │  e é de voz? senão erro              │
   │──────────────────────────────────>│  cap por CANAL, não por conexão      │
   │                                   │  hub.join_voice(canal, B)           │
   │<─── {"type":"voice_presence",     │  broadcast voice_joined             │
   │       channel_id, peers:[A,B]} ───│──────────────────────> (A recebe)   │
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

O server BoraCall fica como **control plane** (auth + metadados de servidor e canal) e o SFU é
adicionado como daemon separado. A superfície WS atual não precisa mudar — basta
um flag de feature `{"use_sfu": true}` no `presence` que diz aos clientes pra
conectarem no SFU em vez de abrirem `RTCPeerConnection`s diretos.

---

## Protocolo WebSocket (JSON por frame de texto)

Endpoint: `GET /ws/servers/{slug}`. Uma conexão por servidor.

### Cliente → servidor (`ClientMsg`)

```jsonc
{"type": "join_voice",  "channel_id": "<uuid>"}   // sair do anterior é implícito
{"type": "leave_voice"}
{"type": "offer",       "to": "<uuid>", "sdp": "..."}
{"type": "answer",      "to": "<uuid>", "sdp": "..."}
{"type": "ice",         "to": "<uuid>", "candidate": {...}}
{"type": "mute",        "muted": true}
{"type": "speaking",    "level": 0.42}            // 0..1, coalescido no cliente
{"type": "typing",      "channel_id": "<uuid>"}
{"type": "leave"}
{"type": "ping"}
```

### Servidor → cliente (`ServerMsg`)

```jsonc
{"type": "voice_state",     "channels": [{"channel_id":"...","peers":[...]}]}  // snapshot ao conectar
{"type": "voice_presence",  "channel_id": "...", "peers": [...]}
{"type": "voice_joined",    "channel_id": "...", "peer": {...}}
{"type": "voice_left",      "channel_id": "...", "user_id": "..."}
{"type": "offer",           "from": "<uuid>", "sdp": "..."}
{"type": "answer",          "from": "<uuid>", "sdp": "..."}
{"type": "ice",             "from": "<uuid>", "candidate": {...}}
{"type": "mute",            "channel_id": "...", "user_id": "...", "muted": true}
{"type": "speaking",        "channel_id": "...", "user_id": "...", "level": 0.4}
{"type": "message",         "channel_id": "...", "message": {...}}
{"type": "message_updated", "channel_id": "...", "message": {...}}
{"type": "message_deleted", "channel_id": "...", "message_id": "..."}
{"type": "typing",          "channel_id": "...", "user_id": "..."}
{"type": "error",           "message": "..."}
{"type": "pong"}
```

### Roteamento

Todo evento entra num `Envelope` com três campos que decidem quem recebe:

| campo | efeito |
|---|---|
| `origin` | quem enviou — **não** recebe eco do próprio evento |
| `target` | quando presente, só esse usuário entrega (SDP, ICE, erro) |
| `scope` | quando presente, só quem está **naquele canal de voz** entrega |

O `scope` é o que impede o pulso de "estou falando", que roda a ~10 Hz, de chegar
em todo mundo do servidor. Mensagem de texto vai sem escopo, porque quem está
com outro canal aberto precisa saber que chegou algo (é o que alimenta o
não-lido).

A regra está isolada em `should_deliver()`, que é função pura e tem teste.

### Autorização dentro do socket

- **Handshake**: `Sec-WebSocket-Protocol: bc.v1, token.<jwt>`. O JWT vai como
  subprotocolo e nunca como query param — URL vaza em log de proxy, histórico de
  browser e header `Referer`. Só membro do servidor conecta; quem não é recebe
  **404** e não 403, pra não confirmar que o servidor existe.
- **`join_voice`** confere que o canal existe, é daquele servidor e é de voz —
  senão bastaria mandar um uuid qualquer pra entrar num canal de texto ou de
  outro servidor.
- **SDP e ICE só trafegam entre pares do mesmo canal de voz.** Sem essa checagem,
  qualquer membro forçaria negociação WebRTC com qualquer outro, inclusive com
  quem não entrou em call nenhuma. ICE fora do canal é descartado em silêncio:
  chega aos borbotões, e um erro por candidato viraria enxurrada.

### A escrita é HTTP; o aviso é WebSocket

Mensagem de texto é persistida por `POST /api/channels/{slug}/messages` e só
então publicada no hub. Publicar **depois do commit** garante que quem receber o
evento e for buscar o histórico já encontra a mensagem. A publicação é
best-effort de propósito: a resposta HTTP já é o recibo do usuário, e a escrita
não pode falhar porque o broadcast falhou.

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
              ┌──────────────────┼───────────────────┬──────────────────┐
              ▼                  ▼                   ▼                  ▼
   ┌──────────────────┐ ┌──────────────────┐ ┌──────────────┐ ┌────────────────┐
   │     servers      │ │  server_members  │ │   messages   │ │   otp_codes    │
   │──────────────────│ │──────────────────│ │──────────────│ │────────────────│
   │ id uuid PK       │<│ server_id FK     │ │ id uuid PK   │ │ purpose        │
   │ slug text UQ     │ │ user_id   FK     │ │ channel_id FK│ │   verify|reset │
   │ name             │ │ role owner|member│ │ user_id   FK │ │ email  citext  │
   │ owner_id  FK     │ │ joined_at        │ │ body         │ │ code_hash      │
   │ created_at       │ │ PK (server,user) │ │ created_at   │ │ expires_at     │
   └────────┬─────────┘ └──────────────────┘ │ edited_at    │ │ attempts       │
            │                                └──────┬───────┘ │ PK(purpose,     │
            ▼                                       │         │    email)      │
   ┌──────────────────┐                             │         └────────────────┘
   │     channels     │                             │
   │──────────────────│                    ┌────────▼─────────┐
   │ id uuid PK       │<───────────────────│  message_reads   │
   │ server_id FK     │                    │──────────────────│
   │ slug text UQ     │  ← global, é o     │ channel_id  FK   │
   │ name             │    link /c/<slug>  │ user_id     FK   │
   │ kind text|voice  │                    │ last_read_msg FK │
   │ position float   │                    │ last_read_at     │
   │ created_at       │                    │ PK (channel,user)│
   └──────────────────┘                    └──────────────────┘
```

Por que cada coisa é do jeito que é:

- **`users.email` é CITEXT** — unique case-insensitive sem `LOWER(...)` espalhado
  por toda query.
- **`channels.slug` é UNIQUE global**, não por servidor: é ele que vira o link
  compartilhável, e `/c/<slug>` precisa resolver sem o slug do servidor junto.
- **`channels.position` é float**, não inteiro. Permite reordenar inserindo no
  meio (nova posição = média dos vizinhos) sem reescrever a coluna inteira a cada
  arrastar.
- **UNIQUE em `(server_id, kind, lower(name))`** — `# geral` de texto e
  `🔊 Geral` de voz convivem; dois `# geral`, não.
- **`messages` ordena por `(created_at, id)`**, nunca pelo id sozinho: o id é
  UUID v4, aleatório, e não ordena por tempo. O id entra só como desempate entre
  duas mensagens do mesmo instante. É esse par que a paginação por cursor compara
  como row value, resolvido pelo índice `messages_channel_cursor_idx`.
- **`message_reads` guarda o timestamp além do id** porque a contagem de
  não-lidos compara por tempo — com UUID v4 não dá pra perguntar "mais novo que".
- **`otp_codes.code_hash`** guarda hash, não o código: um dump do banco não
  entrega o código de ninguém. E `attempts` existe porque um código de 6 dígitos
  cai em 10⁶ chutes, e sem contador nada percebe a varredura.

**Um relógio só.** Todo timestamp comparável vem do `NOW()` do Postgres, nunca do
processo. Misturar os dois já custou um bug: o marcador de leitura usava
`Utc::now()` enquanto as mensagens nasciam com `NOW()`, e alguns centésimos de
skew de NTP deixavam mensagens "no futuro" — o contador de não-lidos não zerava.

---

## TURN e credencial efêmera

STUN só resolve NAT cone. Quem está atrás de **NAT simétrica** — boa parte das
redes corporativas, alguns 4G/5G, hotel — não fecha o candidato P2P, e a chamada
falha **em silêncio**: o app conecta no signaling, mostra o par no canal, e nenhum
áudio passa. É o pior tipo de falha, porque parece que funcionou.

O relay é um coturn ao lado do backend. A credencial **não** vai embutida no app:

```
GET /api/ice   (autenticado)
  → { ice_servers: [ {urls:[stun…]},
                     {urls:[turn…], username, credential} ], ttl: 3600 }

username   = "<unix-de-validade>:<user-id>"
credential = base64( HMAC-SHA1( segredo, username ) )
```

É o mecanismo `use-auth-secret` do coturn: ele refaz a mesma conta com o mesmo
segredo, sem banco de usuários e sem chamada entre os dois serviços.

Usuário e senha fixos no bundle seriam extraídos do binário em cinco minutos, e
aí qualquer um usa o relay de graça. Com credencial assinada por usuário e com
validade, dá pra cortar um abusador sozinho — e **rotacionar o segredo invalida
tudo sem publicar versão nova do desktop**, porque quem decide passa a ser o
servidor.

Sem `BC_TURN_SECRET` configurado, o endpoint devolve só STUN em vez de uma
credencial que o coturn vai recusar. Falhar explícito é melhor que falhar depois.

**Como se verifica que o relay funciona de verdade:** `iceTransportPolicy:
"relay"` faz o navegador descartar todo candidato direto. Se o áudio passa assim,
passou pelo relay. É o que `RELAY_ONLY=1 node scripts/smoke-audio.mjs` faz, e ele
confere o **tipo do candidato** vencedor, não só os bytes — sem isso o teste
passaria com conexão direta e daria falsa sensação de que o TURN está de pé.

---

## Limite de requisições

Duas chaves, porque protegem coisas diferentes:

| chave | onde | contém |
|---|---|---|
| IP | rotas de auth | força bruta de senha, criação em massa de conta, chute de OTP |
| e-mail de destino | rotas que enviam e-mail | encher a caixa de uma vítima, queimar a cota do provedor |

O segundo existe porque **um limite por IP não impede trocar de IP**. E
`/auth/me` fica de fora dos dois: é leitura da própria sessão, chamada no boot e
a cada troca de tela — limitar quebraria uso legítimo.

Balde de fichas, não janela fixa: janela fixa deixa passar o dobro na virada,
porque o pico do fim de uma janela emenda no começo da seguinte.

**O IP vem do `X-Real-IP`, não do `X-Forwarded-For`.** O nginx sobrescreve o
primeiro com o endereço já resolvido; o segundo carrega junto o que o cliente
mandou, e dá pra forjar. Errar isso teria um segundo modo de falha pior: se a
extração falhasse em silêncio, todo mundo cairia no mesmo balde e a API inteira
ficaria limitada como se fosse um cliente só.

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
4. **Postgres read replica** quando leitura (auth, resolver slug de canal) virar gargalo.

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
  - `bc_active_servers`
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
