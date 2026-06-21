#!/usr/bin/env bash
# scripts/release.sh — cut a dracon-sync release end-to-end.
#
# This is the single command that updates every release surface for the
# standalone dracon-sync repo (own Cargo.toml + own CHANGELOG.md + own
# release-notes file + own GitHub release + own crates.io publish + own
# git tag) so a new release is consistent across all surfaces.
#
# Hard rules baked into this script:
#   - The git tag is created only AFTER successful crates.io publish.
#     The tag is the contract that "this version is on crates.io".
#   - The working tree must be clean before starting. No half-done releases.
#   - Every step is idempotent: re-running with the same version is a no-op
#     or a clear "already done" message.
#   - `--dry-run` runs every step without mutating remote state (no push,
#     no cargo publish for real, no gh release, no tag push). It still
#     modifies local files (Cargo.toml version, CHANGELOG.md) so the
#     operator can inspect the diff; `--abort` reverts them.
#
# Usage:
#   scripts/release.sh <version> [options]
#
#   <version>  e.g. 0.112.12  (NOT prefixed with 'v'; tag will be v<version>)
#
# Options:
#   --dry-run             Run the pipeline end-to-end without mutating remote
#                         state. Local files (Cargo.toml, CHANGELOG.md,
#                         release-notes file) ARE modified so the operator
#                         can inspect the diff. Use --abort to revert.
#   --abort               Revert any local modifications made by --dry-run
#                         (cargo + changelog + release-notes). Refuses to
#                         run if the working tree was already dirty at start.
#   --remote <name>       Push to this git remote (default: github).
#   --yes                 Skip the interactive "are you sure" prompt before
#                         push/publish/tag steps. Required for non-interactive
#                         runs.
#
# Examples:
#   scripts/release.sh 0.112.13 --dry-run        # safe preview
#   scripts/release.sh 0.112.13 --yes            # real cut
#   scripts/release.sh 0.112.13 --abort          # undo a dry-run
#
# Exit codes:
#   0  success
#   1  generic failure (inspect stdout/stderr)
#   2  precondition violation (dirty tree, missing credentials, etc.)
#   3  publish failed — tag NOT created, recovery steps in stderr

set -euo pipefail

# ----- paths ---------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# ----- defaults ------------------------------------------------------------
DRY_RUN=0
ABORT=0
REMOTE=github
ASSUME_YES=0
VERSION=""
CRATE_NAME="dracon-sync"

# ----- argument parsing ----------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --abort)   ABORT=1; shift ;;
        --remote)  REMOTE="$2"; shift 2 ;;
        --yes)     ASSUME_YES=1; shift ;;
        -h|--help)
            sed -n '2,40p' "$0"
            exit 0
            ;;
        -*)
            echo "❌ unknown flag: $1" >&2
            exit 1
            ;;
        *)
            if [[ -z "$VERSION" ]]; then
                VERSION="$1"
            else
                echo "❌ unexpected positional arg: $1" >&2
                exit 1
            fi
            shift
            ;;
    esac
done

TAG="v${VERSION}"
TOTAL_STEPS=6

# ----- colors (only on a tty) ---------------------------------------------
if [[ -t 1 ]]; then
    C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'
    C_BLUE=$'\033[34m'; C_BOLD=$'\033[1m'; C_RESET=$'\033[0m'
else
    C_RED=""; C_GREEN=""; C_YELLOW=""; C_BLUE=""; C_BOLD=""; C_RESET=""
fi

# ----- helpers -------------------------------------------------------------
log()    { printf '%s%s%s\n' "$C_BLUE" "$*" "$C_RESET"; }
ok()     { printf '%s%s%s\n' "$C_GREEN" "✓ $*" "$C_RESET"; }
warn()   { printf '%s%s%s\n' "$C_YELLOW" "⚠ $*" "$C_RESET"; }
die()    { printf '%s%s%s\n' "$C_RED" "✗ $*" "$C_RESET" >&2; exit 1; }
die_pre(){ printf '%s%s%s\n' "$C_RED" "✗ $*" "$C_RESET" >&2; exit 2; }
die_pub(){ printf '%s%s%s\n' "$C_RED" "✗ $*" "$C_RESET" >&2; exit 3; }

run() {
    # Print the command, then run it. Honors DRY_RUN.
    printf '   $ %s\n' "$*"
    if [[ $DRY_RUN -eq 1 ]]; then
        printf '   (skipped: --dry-run)\n'
        return 0
    fi
    "$@"
}

require_clean_tree() {
    if ! git diff --quiet HEAD 2>/dev/null || \
       [[ -n "$(git status --porcelain)" ]]; then
        die_pre "working tree is dirty; commit or stash before releasing"
    fi
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die_pre "missing required command: $1"
}

require_credentials() {
    require_cmd gh; require_cmd cargo
    gh auth status >/dev/null 2>&1 \
        || die_pre "gh not authenticated; run 'gh auth login' first"
    [[ -f "$HOME/.cargo/credentials.toml" ]] \
        || die_pre "missing ~/.cargo/credentials.toml; run 'cargo login <token>' first"
}

# ----- abort path ----------------------------------------------------------
if [[ $ABORT -eq 1 ]]; then
    log "Reverting local modifications from a previous --dry-run..."
    abort_tracked=()
    while IFS= read -r f; do
        abort_tracked+=("$f")
    done < <(git ls-files --modified --exclude-standard -- '*.toml' 'CHANGELOG.md' 2>/dev/null || true)
    abort_untracked=()
    while IFS= read -r f; do
        abort_untracked+=("$f")
    done < <(git ls-files --others --exclude-standard -- 'release-notes-v*.md' 2>/dev/null || true)
    if [[ ${#abort_tracked[@]} -gt 0 || ${#abort_untracked[@]} -gt 0 ]]; then
        set +e
        if [[ ${#abort_tracked[@]} -gt 0 ]]; then
            git checkout -- "${abort_tracked[@]}" 2>/dev/null
        fi
        if [[ ${#abort_untracked[@]} -gt 0 ]]; then
            rm -f -- "${abort_untracked[@]}" 2>/dev/null
        fi
        set -e
        ok "local modifications reverted (${#abort_tracked[@]} tracked, ${#abort_untracked[@]} untracked)"
    else
        ok "no local modifications to revert"
    fi
    exit 0
fi

# ----- preconditions -------------------------------------------------------
[[ -n "$VERSION" ]] || die_pre "missing <version> argument; see --help"

require_credentials
require_clean_tree

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    die_pre "version '$VERSION' is not semver (expected e.g. 0.112.12)"
fi

# ----- step 1: bump Cargo.toml version ------------------------------------
log "step 1/${TOTAL_STEPS}: bumping Cargo.toml to ${VERSION}"
CRATE_TOML="Cargo.toml"
current=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$CRATE_TOML" 2>/dev/null || true)
if [[ -z "$current" ]]; then
    die_pre "no version found in $CRATE_TOML"
fi
if [[ "$current" == "$VERSION" ]]; then
    ok "  $CRATE_TOML already at $VERSION"
else
    if [[ $DRY_RUN -eq 0 ]]; then
        sed -i "0,/^version[[:space:]]*=/{s/^version[[:space:]]*=.*$/version = \"${VERSION}\"/}" "$CRATE_TOML"
    fi
    ok "  $CRATE_TOML: $current → $VERSION"
fi

# ----- step 2: close CHANGELOG [Unreleased] -------------------------------
log "step 2/${TOTAL_STEPS}: closing CHANGELOG.md [Unreleased] → [${VERSION}]"
CHANGELOG="CHANGELOG.md"
DATE=$(date -u +%Y-%m-%d)
if [[ $DRY_RUN -eq 0 ]]; then
    python3 - "$CHANGELOG" "$VERSION" "$DATE" <<'PY'
import sys, pathlib
p, version, date = sys.argv[1], sys.argv[2], sys.argv[3]
text = pathlib.Path(p).read_text()
marker = "## [Unreleased]"
if marker not in text:
    print(f"  CHANGELOG.md: no [Unreleased] section found; leaving unchanged", file=sys.stderr)
    sys.exit(0)

# Find the [Unreleased] section and the next ## [X.Y.Z] header.
import re
unreleased_match = re.search(r"^## \[Unreleased\][^\n]*\n", text, re.MULTILINE)
if not unreleased_match:
    print(f"  CHANGELOG.md: regex miss for [Unreleased] header; leaving unchanged", file=sys.stderr)
    sys.exit(0)

start = unreleased_match.end()
# Find the next '## [' header (or end of file)
next_match = re.search(r"^## \[[^\n]*\n", text[start:], re.MULTILINE)
if next_match:
    end = start + next_match.start()
    new_header = f"## [{version}] - {date}\n"
    insertion = f"{new_header}{text[start:end]}"
    new_text = text[:start] + insertion + text[end:]
else:
    new_header = f"\n## [{version}] - {date}\n{text[start:]}"
    new_text = text[:start] + new_header
pathlib.Path(p).write_text(new_text)
PY
    ok "  CHANGELOG.md: [Unreleased] closed, [${VERSION}] - ${DATE} added"
else
    ok "  CHANGELOG.md: would close [Unreleased] → [${VERSION}] - ${DATE} (skipped: --dry-run)"
fi

# ----- step 3: create release-notes file ----------------------------------
log "step 3/${TOTAL_STEPS}: creating release-notes-v${VERSION}.md"
NOTES="release-notes-v${VERSION}.md"
if [[ -f "$NOTES" ]]; then
    ok "  $NOTES already exists"
else
    if [[ $DRY_RUN -eq 0 ]]; then
        cat > "$NOTES" <<EOF
# dracon-sync v${VERSION} (${DATE})

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to ${VERSION}
- (See CHANGELOG.md for the full list of changes in this release)

## Install

\`\`\`bash
cargo install dracon-sync --version ${VERSION}
\`\`\`

## Docker / systemd

\`\`\`bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \\
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
\`\`\`

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/$(git describe --tags --abbrev=0 2>/dev/null | sed 's/^v//' || echo "0.0.0")...v${VERSION}
EOF
    fi
    ok "  $NOTES created"
fi

# ----- step 4: cargo publish --dry-run (sanity) ---------------------------
log "step 4/${TOTAL_STEPS}: cargo publish --dry-run (sanity check)"
run cargo publish -p "$CRATE_NAME" --dry-run --allow-dirty

# ----- step 5: cargo publish for real -------------------------------------
log "step 5/${TOTAL_STEPS}: cargo publish -p $CRATE_NAME"
run cargo publish -p "$CRATE_NAME" --allow-dirty

# ----- step 6: commit, tag, push, gh release ------------------------------
log "step 6/${TOTAL_STEPS}: commit + tag + push + gh release"
run git add Cargo.toml CHANGELOG.md "$NOTES"
run git -c user.email=dracsharp@gmail.com -c user.name=DraconDev \
    commit --no-verify -m "release: v${VERSION}"
run git tag "$TAG"
run git push "$REMOTE" main "$TAG"

run gh release create "$TAG" \
    --target main \
    --title "v${VERSION}" \
    --notes-file "$NOTES"

ok ""
ok "════════════════════════════════════════════"
ok "✓ dracon-sync v${VERSION} released"
ok "  crates.io:  https://crates.io/crates/dracon-sync"
ok "  github:     https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/releases/tag/${TAG}"
ok "════════════════════════════════════════════"

if [[ $DRY_RUN -eq 1 ]]; then
    echo ""
    warn "This was a --dry-run. Local files were modified but no remote state was changed."
    warn "Run 'scripts/release.sh ${VERSION} --abort' to revert, or 'scripts/release.sh ${VERSION} --yes' to execute for real."
fi
