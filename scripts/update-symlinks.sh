#!/usr/bin/env bash
# Aponta os nomes estáveis de download pra versão mais recente de cada bundle.
#
# A landing linka nomes fixos (BoraCall-macos-arm64.dmg, BoraCall-linux.deb…)
# pra não precisar de deploy a cada release. Este script recria os symlinks
# depois que os arquivos novos sobem.
#
# Roda no VPS, em /var/www/boracall/downloads. Instalado em
# /usr/local/bin/boracall-update-symlinks pelo workflow de release.
#
# Histórico: os padrões de .deb e .rpm procuravam "bora-call_*" (com hífen),
# mas o Tauri gera "BoraCall_*". Nunca casaram, os symlinks nunca existiram, e
# dois links da landing responderam 404 desde sempre. Por isso o script agora
# avisa quando um padrão não encontra nada, em vez de seguir em silêncio.

set -uo pipefail   # sem -e de propósito: um padrão sem match não é motivo pra abortar

DIR="${1:-/var/www/boracall/downloads}"
cd "$DIR" || { echo "erro: $DIR não existe" >&2; exit 1; }

faltando=0

link() {
  local pat="$1" dest="$2"
  local f
  # shellcheck disable=SC2086 — o glob precisa expandir
  f=$(ls -t $pat 2>/dev/null | head -1)
  if [ -n "$f" ]; then
    ln -sfn "$f" "$dest"
    echo "  ok      $dest -> $f"
  else
    echo "  FALTA   $dest (nenhum arquivo casa com '$pat')"
    faltando=$((faltando + 1))
  fi
}

echo "symlinks em $DIR:"
link "BoraCall_*_aarch64.dmg"        BoraCall-macos-arm64.dmg
link "BoraCall_*_x64.dmg"            BoraCall-macos-intel.dmg
link "BoraCall_*_amd64.AppImage"     BoraCall-linux.AppImage
link "BoraCall_*_amd64.deb"          BoraCall-linux.deb
link "BoraCall-*.x86_64.rpm"         BoraCall-linux.rpm
link "BoraCall_*_x64-setup.exe"      BoraCall-windows.exe
link "BoraCall_*_x64_en-US.msi"      BoraCall-windows.msi

chown -h www-data:www-data ./BoraCall-* 2>/dev/null || true

if [ "$faltando" -gt 0 ]; then
  echo "$faltando link(s) sem arquivo — a landing vai responder 404 neles" >&2
  exit 1
fi
echo "todos os links resolvidos"
