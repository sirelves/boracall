#!/usr/bin/env bash
# BoraCall — dev local end-to-end.
#
#   sobe Postgres via docker compose
#   inicia boracall-server em 127.0.0.1:3030 (migrations rodam no boot)
#   reescreve dist/env.js pra apontar pro localhost (restaurado no exit)
#   abre `npm run tauri dev`
#
# Resend API key opcional: se ~/.config/boracall/dev.env existir e definir
# BC_RESEND_API_KEY, os e-mails saem de verdade. Senão ficam só no log do server.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEV_ENV_FILE="$HOME/.config/boracall/dev.env"
ENV_JS="$ROOT/dist/env.js"
ENV_JS_BACKUP="$ROOT/dist/env.js.devbak"
SERVER_LOG="$ROOT/target/dev-server.log"
SERVER_PID_FILE="$ROOT/target/dev-server.pid"

mkdir -p target

# ---- cleanup ----------------------------------------------------------------
cleanup() {
  echo
  echo "[dev-local] cleanup…"
  if [[ -f "$SERVER_PID_FILE" ]]; then
    local pid
    pid=$(cat "$SERVER_PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
      echo "[dev-local] parando boracall-server (pid $pid)"
      kill "$pid" 2>/dev/null || true
      sleep 0.5
      kill -9 "$pid" 2>/dev/null || true
    fi
    rm -f "$SERVER_PID_FILE"
  fi
  if [[ -f "$ENV_JS_BACKUP" ]]; then
    echo "[dev-local] restaurando dist/env.js"
    mv "$ENV_JS_BACKUP" "$ENV_JS"
  fi
  echo "[dev-local] Postgres segue rodando. Derruba manual com: docker compose stop"
}
trap cleanup EXIT INT TERM

# ---- Postgres ---------------------------------------------------------------
if ! docker info >/dev/null 2>&1; then
  echo "[dev-local] docker não está rodando. Inicia o Docker Desktop / colima e tenta de novo."
  exit 1
fi

# Checa se postgres já está respondendo — evita depender de `docker compose up -d`
# (alguns setups do docker CLI não aceitam a flag -d nesse contexto).
if pg_isready -h 127.0.0.1 -p 5432 -U boracall -d boracall >/dev/null 2>&1; then
  echo "[dev-local] Postgres já está pronto em 127.0.0.1:5432"
else
  echo "[dev-local] subindo Postgres"
  if ! docker compose up -d postgres >/dev/null 2>&1; then
    echo "[dev-local]   'docker compose up -d' falhou — tentando 'docker-compose up -d'"
    docker-compose up -d postgres >/dev/null 2>&1 || true
  fi
  # Se mesmo assim não subiu, força via run/start no container já existente.
  if ! docker ps --format '{{.Names}}' | grep -q '^boracall-postgres$'; then
    docker start boracall-postgres >/dev/null 2>&1 || true
  fi

  echo -n "[dev-local] aguardando Postgres ficar pronto"
  for i in {1..30}; do
    if pg_isready -h 127.0.0.1 -p 5432 -U boracall -d boracall >/dev/null 2>&1 \
       || docker exec boracall-postgres pg_isready -U boracall -d boracall >/dev/null 2>&1; then
      echo " ok"
      break
    fi
    echo -n "."
    sleep 1
    if [[ $i -eq 30 ]]; then
      echo
      echo "[dev-local] Postgres não ficou pronto em 30s — aborta"
      exit 1
    fi
  done
fi

# ---- env --------------------------------------------------------------------
export DATABASE_URL="postgres://boracall:boracall@127.0.0.1:5432/boracall"
export BC_BIND="127.0.0.1:3030"
export BC_JWT_SECRET="${BC_JWT_SECRET:-dev-jwt-secret-minimo-24-caracteres-ok}"
export BC_JWT_TTL_DAYS="${BC_JWT_TTL_DAYS:-30}"
export BC_CORS_ANY="1"
export RUST_LOG="${RUST_LOG:-boracall_server=debug,tower_http=info,sqlx=warn}"

if [[ -f "$DEV_ENV_FILE" ]]; then
  echo "[dev-local] carregando overrides de $DEV_ENV_FILE"
  # shellcheck disable=SC1090
  set -a; source "$DEV_ENV_FILE"; set +a
fi

if [[ -z "${BC_RESEND_API_KEY:-}" ]]; then
  echo "[dev-local] BC_RESEND_API_KEY vazio — e-mails só serão logados (não enviados)"
  echo "[dev-local]   pra enviar de verdade, cria $DEV_ENV_FILE com:"
  echo "[dev-local]     BC_RESEND_API_KEY=re_xxx"
  echo "[dev-local]     BC_EMAIL_FROM=BoraCall <oi@boracall.com>"
fi

# ---- env.js pra localhost ---------------------------------------------------
cp "$ENV_JS" "$ENV_JS_BACKUP"
cat > "$ENV_JS" <<'EOF'
// dev-local: overridden by scripts/dev-local.sh — restaurado no exit.
(function () {
  window.BC_API_URL    = "http://127.0.0.1:3030";
  window.BC_PUBLIC_URL = "http://127.0.0.1:1420";  // tauri dev server
})();
EOF

# ---- backend ---------------------------------------------------------------
echo "[dev-local] iniciando boracall-server (log em $SERVER_LOG)"
: > "$SERVER_LOG"
cargo run -p boracall-server >>"$SERVER_LOG" 2>&1 &
echo $! > "$SERVER_PID_FILE"

echo -n "[dev-local] aguardando /api/health"
for i in {1..60}; do
  if curl -sf http://127.0.0.1:3030/api/health >/dev/null 2>&1; then
    echo " ok"
    break
  fi
  echo -n "."
  sleep 1
  if [[ $i -eq 60 ]]; then
    echo
    echo "[dev-local] server não subiu em 60s — olha $SERVER_LOG"
    exit 1
  fi
done

# ---- tauri dev (foreground) ------------------------------------------------
echo "[dev-local] abrindo tauri dev — ctrl+c pra sair"
echo "[dev-local] server logs: tail -f $SERVER_LOG"
npm run tauri dev
