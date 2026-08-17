# Contribuindo pro BoraCall

Valeu por considerar contribuir! Este guia cobre o essencial pra não perder tempo.

---

## Antes de abrir um PR grande

Abra uma **issue** primeiro descrevendo a ideia. Mudanças de arquitetura
(ex: adicionar um broker, trocar o signaling, integrar SFU) geralmente
exigem alinhamento antes de virar código. Bug fixes pequenos e refactors
localizados podem vir direto como PR.

Áreas onde ajuda é especialmente bem-vinda:

- **TURN server** integration + docs (coturn config)
- **SFU opcional** (LiveKit ou mediasoup) ao lado do Rust
- **Testes E2E** com dois webviews headless falando entre si
- **QA de Windows e Linux** — codecs WebRTC variam por webview
- **Acessibilidade** — screen reader no fluxo de call
- **Translations** (hoje só PT-BR)
- **Observabilidade** — OpenTelemetry + metrics Prometheus

---

## Setup de desenvolvimento

Siga [README.md § Rodando local](./README.md#rodando-local).

Resumo:

```bash
cp .env.example .env              # ajuste BC_JWT_SECRET
docker compose up -d              # Postgres 16
cargo run -p boracall-server      # backend
npm install && npm run dev        # app Tauri com hot reload
```

---

## Convenções

### Rust

- **rustfmt default** — rode `cargo fmt` antes de commitar.
- **clippy limpo** — `cargo clippy --workspace --all-targets -- -D warnings`.
- **Queries SQL** sempre via `sqlx::query!` / `query_as!` (não `query_unchecked`) —
  o projeto depende do compile-time check.
- **Cache offline do sqlx**: o diretório `.sqlx/` é **commitado**. Com ele,
  `SQLX_OFFLINE=true cargo build -p boracall-server` compila sem Postgres nenhum
  (é assim que o CI roda). **Sempre que você criar ou alterar uma query**, regenere
  com o banco no ar e commite junto:

  ```bash
  cargo sqlx prepare --workspace -- -p boracall-server
  ```

  O job `sqlx-cache` do CI roda `cargo sqlx prepare --check` e falha se você esquecer.
- **Erros novos** entram em `error.rs` com variant tipada, nunca `anyhow::Error`
  escapando pra handler.
- **Logs**: `tracing::info!` / `warn!` / `error!`. Não use `println!`.
- **Comentários** explicam *por quê*, não *o quê*. Se tiver dúvida, menos é mais.

### JavaScript / JSX

- Sem build step — edita, reload. Sintaxe = ES2020 + JSX via Babel standalone.
- **Não adicione bundler** sem discussão prévia (isso muda o shape do projeto).
- API globals: `window.api`, `window.Realtime`, `window.WebRTCMesh`, `window.desktop`.
  Não polua mais `window`.

### Commits

Mensagens em PT-BR ou EN, como preferir. Formato livre, mas prefira **imperativo**
e uma linha curta de resumo:

```
feat: add refresh token endpoint
fix: webkit2gtk permission hook crash on wayland
docs: expand architecture sequence diagram
```

Squash antes de merge é incentivado pra PRs que fizeram muitas idas e vindas.

---

## Segurança

Achou vulnerabilidade?

- **Não** abra issue pública.
- Mande um e-mail direto com PoC mínima. O endereço está no `package.json` /
  perfil do repo.
- Resposta em até 72h. CVE + crédito no changelog.

---

## Licença

Ao contribuir, você concorda que seu código será publicado sob [MIT](./LICENSE).
