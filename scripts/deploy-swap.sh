#!/usr/bin/env bash
# Troca o binário do boracall-server pelo que acabou de subir, reinicia e
# verifica. Volta o anterior se o health não responder.
#
# Roda NO VPS, chamado pelo workflow deploy-backend.yml. Fica num arquivo em vez
# de heredoc dentro do YAML porque heredoc indentado não fecha em bash, e porque
# assim dá pra rodar à mão quando o CI estiver indisponível:
#
#   scp target/release/boracall-server root@vps:/opt/boracall/bin/boracall-server.new
#   scp scripts/deploy-swap.sh root@vps:/tmp/ && ssh root@vps bash /tmp/deploy-swap.sh

set -euo pipefail

DEST="${DEST:-/opt/boracall/bin/boracall-server}"
SERVICO="${SERVICO:-boracall-server}"
HEALTH="${HEALTH:-http://127.0.0.1:3030/api/health}"
VERSION_URL="${VERSION_URL:-http://127.0.0.1:3030/api/version}"
TENTATIVAS="${TENTATIVAS:-20}"

if [ ! -f "$DEST.new" ]; then
  echo "erro: $DEST.new não existe — o upload falhou?" >&2
  exit 1
fi

# Guarda o que está rodando pra poder voltar.
if [ -f "$DEST" ]; then
  cp -f "$DEST" "$DEST.prev"
  echo "binário atual guardado em $DEST.prev"
fi

chmod 755 "$DEST.new"
mv -f "$DEST.new" "$DEST"
echo "binário trocado"

systemctl restart "$SERVICO"

# O boot roda as migrations antes de escutar, então o health demora um pouco.
for i in $(seq 1 "$TENTATIVAS"); do
  sleep 1
  if curl -fsS -m 3 "$HEALTH" >/dev/null 2>&1; then
    echo "health ok na tentativa $i"
    echo "no ar: $(curl -fsS -m 3 "$VERSION_URL")"
    # Garante que sobe sozinho no boot — foi assim que a API sumiu da última vez.
    systemctl is-enabled --quiet "$SERVICO" || {
      echo "serviço estava disabled; habilitando"
      systemctl enable "$SERVICO"
    }
    exit 0
  fi
done

echo "::error::health não respondeu em ${TENTATIVAS}s — revertendo"
journalctl -u "$SERVICO" -n 40 --no-pager || true

if [ -f "$DEST.prev" ]; then
  mv -f "$DEST.prev" "$DEST"
  systemctl restart "$SERVICO"
  sleep 3
  if curl -fsS -m 3 "$HEALTH" >/dev/null 2>&1; then
    echo "rollback ok — produção voltou pro binário anterior"
  else
    echo "::error::rollback TAMBÉM falhou — a API está fora"
  fi
else
  echo "::error::não havia binário anterior pra voltar"
fi
exit 1
