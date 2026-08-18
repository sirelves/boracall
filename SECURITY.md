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

Algumas limitações conhecidas já têm issue aberta e estão sendo tratadas — as de
**produto** ficam públicas (validade do JWT, assinatura de código dos
instaladores). As de **infraestrutura** ficam em Security Advisory privado, não
em issue.

Se você acha que encontrou algo, relate: no pior caso já sabíamos e respondemos
em um dia.

## Regra: infraestrutura não vai para issue pública

Este repositório é **público**. Descrever configuração de servidor — endereço,
portas abertas, o que está ou não instalado, regras de firewall, nomes de outros
serviços na mesma máquina — em issue ou PR é publicar um mapa para quem quiser
atacar, mesmo quando a intenção é rastrear a correção.

Vale tanto para quem relata quanto para quem mantém:

| onde | o quê |
|---|---|
| Issue pública | *"Endurecer o acesso ao banco de produção"* — o que precisa ser feito |
| Advisory privado | endereço, porta, configuração atual, comando de correção, evidência |

Se você é mantenedor e precisa registrar um achado de infraestrutura: abra o
advisory primeiro, e deixe na issue apenas o título da tarefa com um ponteiro.

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
