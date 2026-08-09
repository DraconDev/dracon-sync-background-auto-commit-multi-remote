#!/usr/bin/env bash
# scripts/verify-install.sh — post-install / pre-release fixture check for a
# dracon-sync binary.
#
# Guards against the 2026-08-08 incident class: a binary built with an
# unpatched dracon-git whose libgit2 status path counts gitignored files
# (e.g. `.pi/`) as untracked, producing phantom untracked counts in
# `dracon-sync repos` (endless-td 294, dracon-platform 48, ...) that can
# never be committed. The trigger was `cargo publish` silently dropping the
# workspace `[patch.crates-io]`, so `cargo install` resolved an old crates.io
# dracon-git. The workspace build and the crates.io-resolved build must BOTH
# report untracked=0 for a repo whose `.pi/` dir is gitignored — this script
# asserts exactly that, behaviorally, against any binary you point it at.
#
# Usage:
#   scripts/verify-install.sh [binary-path]
#
#   binary-path   default: `dracon-sync` (resolved via PATH) — i.e. the
#                 installed binary the operator just put in place.
#
# Exit codes: 0 = fixture clean; 1 = fixture failed (gitignore handling
# broken in the binary under test).
set -euo pipefail

BIN="${1:-dracon-sync}"
if ! command -v "$BIN" >/dev/null 2>&1; then
    echo "✗ binary '$BIN' not found on PATH" >&2
    exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ── fixture repo: a single committed file + a gitignored `.pi/` dir ──────
REPO="$TMP/repo"
mkdir -p "$REPO/.pi"
git -C "$REPO" init -q -b main
git -C "$REPO" config user.email fixture@test.local
git -C "$REPO" config user.name fixture
git -C "$REPO" config core.hooksPath /dev/null
echo "content" > "$REPO/f.txt"
git -C "$REPO" add f.txt
git -C "$REPO" commit -qm init
printf '.pi/\n' > "$REPO/.gitignore"
git -C "$REPO" add .gitignore
git -C "$REPO" commit -qm ignore-rule
for i in 1 2 3; do echo "x" > "$REPO/.pi/shot$i.png"; done

# git CLI ground truth: 0 untracked (the ignore rule applies)
if [[ "$(git -C "$REPO" ls-files --others --exclude-standard | wc -l)" != "0" ]]; then
    echo "✗ fixture self-check failed (git CLI sees untracked files)" >&2
    exit 1
fi

cat > "$TMP/policy.toml" <<EOF
watch_roots = ["$REPO"]
EOF

# ── run the binary under test and read its untracked count ───────────────
# DRACON_SYNC_POLICY wires the fixture policy into the binary under test so
# `repos --json` scans ONLY the fixture repo. (FIXED 2026-08-09, audit HIGH:
# the env was previously unset, so the check asserted rows[0] of the
# operator's REAL fleet — passing only while the fleet happens to be clean
# and false-failing whenever a legitimately-dirty fleet repo is row 0.)
OUT="$(DRACON_SYNC_POLICY="$TMP/policy.toml" "$BIN" repos --json 2>/dev/null || true)"
UNTRACKED="$(printf '%s' "$OUT" | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
    rows = d["rows"] if isinstance(d, dict) and "rows" in d else d
    print(rows[0]["untracked"] if rows else "-1")
except Exception:
    print("-1")
' 2>/dev/null || echo "-1")"
SCANNED="$(printf '%s' "$OUT" | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
    rows = d["rows"] if isinstance(d, dict) and "rows" in d else d
    print(rows[0]["repo"] if rows else "")
except Exception:
    print("")
' 2>/dev/null || echo "")"

if [[ "$SCANNED" != "$REPO" ]]; then
    echo "✗ FAIL: '$BIN' scanned '$SCANNED' — expected the fixture repo '$REPO'." >&2
    echo "  The fixture policy (\$DRACON_SYNC_POLICY=$TMP/policy.toml) was not honored; the" >&2
    echo "  check would silently test the operator's real fleet instead of the fixture." >&2
    exit 1
fi

if [[ "$UNTRACKED" != "0" ]]; then
    echo "✗ FAIL: '$BIN' reports untracked=$UNTRACKED for a repo whose only untracked files are gitignored (.pi/)." >&2
    echo "  Expected 0. This is the 2026-08-08 phantom-untracked incident: the binary was" >&2
    echo "  built against an unpatched dracon-git whose libgit2 status path ignores rules" >&2
    echo "  differently from git CLI. See docs/design/installed-binary-drops-patch-dracon-git-2026-08-08.md." >&2
    exit 1
fi
echo "✓ OK: '$BIN' reports untracked=0 — gitignore handling intact."
