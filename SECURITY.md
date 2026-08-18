# Segurança

## Como relatar

**Não abra issue pública para vulnerabilidade.**

Use o [Security Advisory privado](https://github.com/sirelves/boracall/security/advisories/new).
Se preferir e-mail, o endereço está no perfil do repositório.

Inclua o que tiver: passo a passo, prova de conceito mínima, e o impacto que
você enxerga. Um relato com repro vale mais que um relato completo.

Resposta em até 72h. Se for procedente, crédito no changelog e no advisory —
a menos que você prefira anonimato.

## Versões

O projeto está em `0.x`: só a versão mais recente recebe correção. Não há
backport para versões anteriores.

## O que já sabemos

Estas são limitações conhecidas, com issue aberta. Relatar de novo não é
necessário — mas contribuição para resolvê-las é bem-vinda:

| o quê | issue |
|---|---|
| JWT com validade de 30 dias, sem rotação nem revogação | [#14](https://github.com/sirelves/boracall/issues/14) |
| Instaladores sem assinatura de código (Gatekeeper / SmartScreen) | [#15](https://github.com/sirelves/boracall/issues/15) |
| Porta do Postgres do servidor aberta na internet | [#2](https://github.com/sirelves/boracall/issues/2) |

## Escopo

Vale relatar: qualquer coisa que permita ler dado de outra pessoa, entrar numa
conta alheia, entrar num servidor/canal sem convite, ou derrubar o serviço.

Provavelmente não vale: falta de rate limit em rota que já tem
(veja `BC_RL_*` no `.env.example`), ausência de header de segurança sem impacto
demonstrável, e resultado bruto de scanner automático sem análise.

## O que o produto não promete

O áudio é peer-to-peer: **os participantes de uma chamada enxergam o endereço IP
uns dos outros**, como em qualquer WebRTC sem relay forçado. Quem não quer isso
pode forçar todo o tráfego pelo TURN (`iceTransportPolicy: "relay"`), ao custo de
latência. Isso é característica da arquitetura, não vulnerabilidade.

As mensagens dos canais de texto **não são criptografadas ponta a ponta** — ficam
no banco e quem administra o servidor consegue lê-las.
