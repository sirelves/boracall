<!--
  Snippet pronto pra colar no seu README principal (GitHub profile, portfolio,
  etc). Ajuste o link do repo (`<owner>/BoraCall`) antes de publicar.
  Duas versões: a longa (seção completa) e a curta (card de uma linha).
-->

## 🎙 BoraCall — desktop voice rooms em Rust + Tauri

> Chamadas de voz em grupo, leves, sem enrolação. Mesh WebRTC P2P, server stateless em Rust, cliente desktop cross-platform (macOS / Windows / Linux).

<a href="https://github.com/<owner>/BoraCall">
  <img src="https://github.com/<owner>/BoraCall/raw/main/landing-hero.png" alt="BoraCall" width="70%" />
</a>

**Stack:** Tauri v2 · Rust (axum 0.8 + sqlx) · Postgres 16 · WebRTC · React 18 (no build step) · jemalloc · Argon2id · JWT
**Código:** ~4.4k LOC · 100% open source (MIT) · CI gera 4 bundles nativos em paralelo

### O que tem de interessante

- 🦀 **Backend Rust** com sqlx compile-time checked queries, jemalloc global allocator e `tokio::sync::broadcast` per-room hub
- 🔐 **JWT via `Sec-WebSocket-Protocol` subprotocol** — não via query string (não vaza em logs/histórico)
- 🌐 **WebRTC mesh real** com glare avoidance determinístico (`user_id` lexicográfico menor cria o offer)
- 🖥 **Desktop nativo** via Tauri v2 com auto-updater ed25519-signed, `~8MB` de binário vs ~100MB do Electron
- ⚙️ **CI/CD** com 4 runners paralelos gerando `.dmg` (ARM+Intel) / `.msi` / `.AppImage` / `.deb` / `.rpm`
- 🎨 **Zero build step no frontend** — React 18 + Babel standalone via `<script>`, fontes vendoradas
- 📚 **Documentação completa**: [README](https://github.com/<owner>/BoraCall) · [ARCHITECTURE.md](https://github.com/<owner>/BoraCall/blob/main/ARCHITECTURE.md) · [HANDOFF.md](https://github.com/<owner>/BoraCall/blob/main/HANDOFF.md) (deploy VPS)

[→ Repositório](https://github.com/<owner>/BoraCall) · [→ Arquitetura](https://github.com/<owner>/BoraCall/blob/main/ARCHITECTURE.md) · [→ Landing](https://boracall.com)

---

<!-- Versão curta (card) — se o README principal já tem muita coisa -->

<!--
### BoraCall
**Rust · Tauri v2 · Postgres · WebRTC mesh · React 18**
Chamadas de voz em grupo num app desktop cross-platform. Server stateless em Rust (axum + sqlx), mesh WebRTC P2P até 6 peers, JWT via WS subprotocol, auto-updater ed25519. CI gera 4 bundles nativos em paralelo.
→ [github.com/<owner>/BoraCall](https://github.com/<owner>/BoraCall)
-->
