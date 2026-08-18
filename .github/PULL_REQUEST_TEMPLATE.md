<!--
Obrigado por contribuir com o BoraCall!

Mudança grande? Abra uma issue antes, para alinhar o rumo sem você gastar
tempo à toa. Veja o CONTRIBUTING.md.
-->

## O que este PR faz

<!-- Uma ou duas frases: a mudança e o motivo dela. -->

## Issue relacionada

Closes #

## Checklist

<!-- Marque o que se aplica. Item que não faz sentido no seu PR, risque. -->

- [ ] `cargo fmt -p boracall-server` e `cargo clippy -p boracall-server --all-targets -- -D warnings` limpos
- [ ] `cargo test -p boracall-server` passa
- [ ] **Mexeu em query SQL?** Rodou `cargo sqlx prepare --workspace -- -p boracall-server --all-targets` e commitou o `.sqlx/`
- [ ] Configuração nova por variável de ambiente está no `.env.example`
- [ ] Nenhum segredo, token ou hostname interno no código ou nos comentários
- [ ] Documentação atualizada (`README.md` / `ARCHITECTURE.md` / `CONTRIBUTING.md`) se o comportamento ou a API mudou

## Como você testou

<!--
Diga o que rodou de verdade. Exemplos do que conta:

  cargo test -p boracall-server -- --ignored     # precisa de Postgres
  node scripts/smoke-ws.mjs                      # signaling com dois sockets
  node scripts/smoke-audio.mjs                   # áudio real entre dois navegadores
  RELAY_ONLY=1 node scripts/smoke-audio.mjs      # forçando o TURN

Mexeu no front? Diga em qual sistema abriu o app e o que clicou.
-->
