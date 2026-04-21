#!/usr/bin/env bash
# BoraCall — manual publisher for release artifacts.
#
# When GitHub Actions is not running (e.g. first release, local build, or a hotfix
# rebuilt by hand), use this to push the current local bundles to the VPS and the
# GitHub Release.
#
# Expects the following env vars:
#   DEPLOY_HOST   — VPS hostname or IP
#   DEPLOY_USER   — ssh user with write access to /var/www/boracall/downloads/
#   GITHUB_REPO   — "owner/repo" (optional; skips GH upload if unset)
#   GITHUB_TOKEN  — token with `contents:write` (optional)
#
# Usage:
#   VERSION=0.1.0 scripts/deploy-downloads.sh

set -euo pipefail

VERSION="${VERSION:-0.1.0}"
DEPLOY_HOST="${DEPLOY_HOST:?need DEPLOY_HOST}"
DEPLOY_USER="${DEPLOY_USER:?need DEPLOY_USER}"
DEPLOY_DEST="${DEPLOY_DEST:-/var/www/boracall/downloads}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGE="$(mktemp -d)"
echo "staging in $STAGE"

collect() {
  local pattern="$1"
  local from="$2"
  while IFS= read -r -d '' f; do
    cp "$f" "$STAGE/"
    echo "  + $(basename "$f")"
  done < <(find "$from" -type f -name "$pattern" -print0 2>/dev/null || true)
}

echo "collecting bundles..."
collect '*.dmg'      "$ROOT/target/release/bundle/dmg"
collect '*.dmg'      "$ROOT/target/universal-apple-darwin/release/bundle/dmg"
collect '*.msi'      "$ROOT/target/release/bundle/msi"
collect '*.exe'      "$ROOT/target/release/bundle/nsis"
collect '*.AppImage' "$ROOT/target/release/bundle/appimage"
collect '*.deb'      "$ROOT/target/release/bundle/deb"
collect '*.rpm'      "$ROOT/target/release/bundle/rpm"

if [[ -z "$(ls -A "$STAGE")" ]]; then
  echo "no bundles found — did you run 'npm run build' yet?" >&2
  exit 1
fi

# Stable-named symlinks for boracall.com/downloads/latest/<platform>.
( cd "$STAGE" && shasum -a 256 ./* | tee SHA256SUMS >/dev/null )

echo "publishing to $DEPLOY_USER@$DEPLOY_HOST:$DEPLOY_DEST/"
ssh "$DEPLOY_USER@$DEPLOY_HOST" "mkdir -p $DEPLOY_DEST"
rsync -avz "$STAGE/" "$DEPLOY_USER@$DEPLOY_HOST:$DEPLOY_DEST/"

# Optional: also attach to a GitHub Release.
if [[ -n "${GITHUB_REPO:-}" && -n "${GITHUB_TOKEN:-}" ]]; then
  echo "attaching to GitHub release v$VERSION on $GITHUB_REPO..."
  if ! command -v gh >/dev/null 2>&1; then
    echo "  (skip: gh CLI not installed)"
  else
    gh release view "v$VERSION" --repo "$GITHUB_REPO" >/dev/null 2>&1 || \
      gh release create "v$VERSION" --repo "$GITHUB_REPO" --draft --title "BoraCall v$VERSION" --notes "Manual release."
    gh release upload "v$VERSION" --repo "$GITHUB_REPO" --clobber "$STAGE/"*
  fi
fi

rm -rf "$STAGE"
echo "done."
