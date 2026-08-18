# Contribuindo pro BoraCall

Valeu por considerar contribuir! Este guia cobre o essencial pra não perder tempo.

---

## Antes de abrir um PR grande

Abra uma **issue** primeiro descrevendo a ideia. Mudanças de arquitetura
(ex: adicionar um broker, trocar o signaling, integrar SFU) geralmente
exigem alinhamento antes de virar código. Bug fixes pequenos e refactors
localizados podem vir direto como PR.

### Por onde começar

Procure a etiqueta [`good first issue`](https://github.com/sirelves/boracall/labels/good%20first%20issue):
são tarefas de escopo fechado, com o arquivo e a abordagem já indicados no corpo
da issue.

Áreas onde ajuda é especialmente bem-vinda hoje:

- **QA de Windows e Linux** — os codecs de WebRTC variam por webview, e o
  projeto é desenvolvido no macOS. Achar o que quebra fora dele vale muito.
- **SFU opcional** (LiveKit ou mediasoup) — hoje a chamada é mesh P2P e satura
  acima de ~6 pessoas por canal
- **Acessibilidade** — leitor de tela no fluxo de chamada
- **Tradução** — hoje só PT-BR
- **Observabilidade** — OpenTelemetry, métricas Prometheus

Já resolvidos, não precisa: TURN (existe, com credencial efêmera) e testes
ponta a ponta de voz (`scripts/smoke-*.mjs`).

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

**Só quer mexer no backend sem subir banco?** Dá:

```bash
SQLX_OFFLINE=true cargo build -p boracall-server
SQLX_OFFLINE=true cargo test  -p boracall-server
```

O `.sqlx/` commitado tem o retrato das queries. Banco só é necessário pra rodar
os testes de integração (`-- --ignored`) e pra regerar esse cache.

**Mexendo no front?** O `dist/` é HTML e JSX servidos direto, sem build. Editou,
recarregou. Se preferir o navegador ao app nativo:

```bash
cd dist && python3 -m http.server 5174
```

O `dist/env.js` aponta pro backend local por padrão — não mexa nele achando que
precisa apontar pra produção; o build de release reescreve esse arquivo sozinho.

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
  cargo sqlx prepare --workspace -- -p boracall-server --all-targets
  ```

  O `--all-targets` não é opcional: sem ele o cache ignora as queries que vivem
  dentro de `#[cfg(test)]`, e aí o `cargo test` offline não compila.

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

Mensagens em PT-BR ou EN, como preferir — o projeto é escrito em português,
mas ninguém vai recusar contribuição por causa do idioma do commit. Formato livre, mas prefira **imperativo**
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

---

## Verificação de voz

Dois scripts, sem dependência de framework de teste, que cobrem o que teste de
unidade não alcança:

```bash
# 1) signaling: dois sockets reais trocando offer/answer/ICE
cargo run -p boracall-server           # num terminal
node scripts/smoke-ws.mjs              # noutro

# 2) áudio: dois navegadores com microfone sintético, RTP de verdade
(cd dist && python3 -m http.server 5174)
npm i playwright && npx playwright install chromium
node scripts/smoke-audio.mjs

# 3) o mesmo, forçando todo o tráfego pelo TURN
RELAY_ONLY=1 node scripts/smoke-audio.mjs
```

O terceiro é o único jeito de provar que o relay funciona sem estar atrás de uma
NAT simétrica de verdade: `iceTransportPolicy: "relay"` descarta candidato
direto, então se o áudio passa, passou pelo coturn. Ele confere o tipo do
candidato vencedor (`relay`) além dos bytes.

Ambos rodam contra outro host via `API=https://api.boracall.com node scripts/...`.

## coturn

O TURN usa credencial efêmera (`use-auth-secret`): o backend assina
`<validade>:<user-id>` com HMAC-SHA1 usando o mesmo `static-auth-secret` do
`/etc/turnserver.conf`, e entrega em `GET /api/ice`. Não existe usuário fixo pra
vazar dentro do bundle do app, e rotacionar o segredo invalida tudo que já foi
entregue — sem publicar versão nova do desktop.
