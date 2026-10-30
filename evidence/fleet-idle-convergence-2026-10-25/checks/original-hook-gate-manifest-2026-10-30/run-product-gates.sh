#!/usr/bin/env bash
# Revision-addressed original-hook/product-gate evidence for every repaired
# repository (2026-10-30, auditor objection 3).
#
# Runs each repository's OWN declared product gates (the same ones its
# pre-push hook runs), captures raw output, and records the exact revision
# under test. Nothing is skipped, softened or substituted.
set -u

REPO=/home/dracon/Dev/browser-extensions-shared
OUT=/home/dracon/Dev/dracon-utilities/dracon-sync/evidence/fleet-idle-convergence-2026-10-25/checks/original-hook-gate-manifest-2026-10-30
mkdir -p "$OUT"

cd "$REPO" || exit 1
REV=$(/run/current-system/sw/bin/git rev-parse HEAD)
TREE=$(/run/current-system/sw/bin/git rev-parse 'HEAD^{tree}')
echo "$REV" > "$OUT/revision.txt"
echo "$TREE" > "$OUT/tree.txt"

run_one() {
  local ws="$1" gate="$2"
  local dir="extensions/standalones/$ws"
  [ -d "$dir" ] || dir="extensions/collections/$ws"
  [ -d "$dir" ] || return 0
  local pkg="$dir/package.json"
  [ -f "$pkg" ] || return 0
  python3 - "$pkg" "$gate" <<'PY' || return 0
import json,sys
d=json.load(open(sys.argv[1]))
sys.exit(0 if sys.argv[2] in d.get('scripts',{}) else 1)
PY
  local base="${ws//\//_}-${gate}"
  ( cd "$dir" && timeout 2400 bun run "$gate" ) > "$OUT/$base.log" 2>&1
  echo "$?" > "$OUT/$base.exit"
  printf '  %-42s exit=%s\n' "$base" "$(cat "$OUT/$base.exit")"
}

echo "revision $REV"
for ws in \
  extensions/standalones/markdown-reader \
  extensions/standalones/youtube-filter \
  extensions/standalones/tabmon \
  extensions/standalones/clean-web \
  extensions/standalones/ai-tts-pro
do
  ws="${ws##extensions/standalones/}"
  for gate in lint typecheck test build; do
    run_one "$ws" "$gate"
  done
done

# The repository's own pre-push hook suite (the hook is a repaired artifact).
if [ -f scripts/git-hooks/pre-push.test.mjs ]; then
  ( cd "$REPO" && timeout 1800 node --test scripts/git-hooks/pre-push.test.mjs ) \
    > "$OUT/repo-pre-push-hook.log" 2>&1
  echo "$?" > "$OUT/repo-pre-push-hook.exit"
  printf '  %-42s exit=%s\n' "repo-pre-push-hook" "$(cat "$OUT/repo-pre-push-hook.exit")"
fi

# Warden's own pre-push suite (the other repaired hook).
if [ -d /home/dracon/Dev/dracon-utilities ]; then
  ( cd /home/dracon/Dev/dracon-utilities \
    && timeout 2400 cargo test -p dracon-warden --locked pre_push_ ) \
    > "$OUT/warden-pre-push.log" 2>&1
  echo "$?" > "$OUT/warden-pre-push.exit"
  printf '  %-42s exit=%s\n' "warden-pre-push" "$(cat "$OUT/warden-pre-push.exit")"
fi

touch "$OUT/DONE"
echo done
