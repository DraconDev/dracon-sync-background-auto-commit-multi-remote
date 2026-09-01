#!/usr/bin/env bash
# scripts/release.sh — cut a dracon-sync release end-to-end.
#
# This command releases the dracon-sync package from the dracon-utilities
# monorepo: it updates the utility's Cargo.toml/CHANGELOG/release notes,
# the monorepo lockfile, crates.io, the monorepo tag, and its GitHub release.
#
# Hard rules baked into this script:
#   - The git tag is created only AFTER successful crates.io publish.
#     The tag is the contract that "this version is on crates.io".
#   - The parent monorepo working tree must be clean before starting. Run
#     this through `dracon-sync maintenance -- ...` to avoid daemon races.
#   - Every step is idempotent: re-running with the same version is a no-op
#     or a clear "already done" message.
#   - `--dry-run` runs every local validation step without mutating remote
#     state (no real publish, push, tag, or GitHub release). It modifies the
#     utility release surfaces and the workspace lockfile so the operator can
#     inspect the diff; `--abort` reverts those changes.
#
# Usage:
#   scripts/release.sh <version> [options]
#
#   <version>  e.g. 0.112.12  (NOT prefixed with 'v'; tag will be v<version>)
#
# Options:
#   --dry-run             Run the pipeline end-to-end without mutating remote
#                         state. Local release surfaces (utility Cargo.toml,
#                         workspace Cargo.lock, CHANGELOG, release notes) ARE
#                         modified so the operator can inspect the diff. Use
#                         --abort to revert.
#   --abort               Revert any local modifications made by --dry-run
#                         (utility Cargo.toml + workspace lock + changelog +
#                         release notes). Refuses to
#                         run if the working tree contains pre-existing
#                         modifications outside those release surfaces
#                         (CORRECTED 2026-08-10, audit LOW: the guard is now
#                         real — the abort path used to run unchecked).
#   --remote <name>       Push to this git remote (default: auto-detect the
#                         github.com remote via scripts/resolve-github-remote.sh).
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
# FIXED 2026-09-01 (post-monorepo conversion 2026-08-22): the previous
# version of this script treated the utility directory as a standalone Git
# repository. The utility directories are now tracked inside the parent
# monorepo and are intentionally ignored for ordinary `git add`, so resolve
# both roots explicitly and force-stage only the release paths below.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"
CRATE_REL="${CRATE_DIR#"$REPO_ROOT"/}"
cd "$REPO_ROOT"

CRATE_TOML="$CRATE_DIR/Cargo.toml"
CHANGELOG="$CRATE_DIR/CHANGELOG.md"
LOCKFILE="$REPO_ROOT/Cargo.lock"

# ----- defaults ------------------------------------------------------------
DRY_RUN=0
ABORT=0
REMOTE=""          # --remote override; empty = auto-detect the github.com remote
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
TOTAL_STEPS=8

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

run_local() {
    # Commands using this helper are local validation steps. They are safe
    # during --dry-run and must still execute so a preview exercises the
    # package and dependency-resolution checks it claims to run.
    printf '   $ %s\n' "$*"
    "$@"
}

require_clean_tree() {
    if ! git diff --quiet HEAD 2>/dev/null || \
       [[ -n "$(git ls-files --others --exclude-standard)" ]] || \
       [[ -n "$(release_note_files)" ]]; then
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

is_release_surface() {
    local path=$1
    [[ "$path" == "$CRATE_REL/Cargo.toml" ||
       "$path" == "$CRATE_REL/CHANGELOG.md" ||
       "$path" == "Cargo.lock" ||
       "$path" == "$CRATE_REL"/release-notes-v*.md ]]
}

release_note_files() {
    # The parent .gitignore intentionally ignores the utility directory, so
    # include ignored-but-untracked release notes when handling --abort.
    git ls-files --others --ignored --exclude-standard -- \
        "$CRATE_REL/release-notes-v*.md" 2>/dev/null || true
}

confirm_remote_mutation() {
    [[ "$DRY_RUN" -eq 1 || "$ASSUME_YES" -eq 1 ]] && return 0
    if [[ ! -t 0 ]]; then
        die_pre "--yes is required for a non-interactive real release"
    fi
    local answer
    if ! read -r -p "Publish ${CRATE_NAME}@${VERSION}, tag ${TAG}, and push to ${REMOTE}? [y/N] " answer; then
        die_pre "release confirmation was not provided"
    fi
    case "${answer,,}" in
        y|yes) ;;
        *) die_pre "release cancelled" ;;
    esac
}

refresh_workspace_lock() {
    # A package version is part of the workspace lockfile. Cargo updates only
    # the affected local package entry here; unlike generate-lockfile this
    # does not discard the monorepo's intentionally pinned dependency graph.
    if ! cargo check -p "$CRATE_NAME" --quiet; then
        die_pre "failed to synchronize the workspace Cargo.lock for $CRATE_NAME@$VERSION"
    fi
    [[ -s "$LOCKFILE" ]] || die_pre "workspace Cargo.lock is missing after cargo check"
    ok "  Cargo.lock synchronized for $CRATE_NAME@$VERSION"
}

# ----- abort path ----------------------------------------------------------
if [[ $ABORT -eq 1 ]]; then
    log "Reverting local modifications from a previous --dry-run..."
    # Refuse to touch operator work outside this crate's release surfaces.
    # `git diff HEAD` includes staged and unstaged tracked changes; the
    # ignored release-note path is checked separately because the monorepo
    # deliberately ignores the utility directory for ordinary additions.
    other_modified=()
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        if ! is_release_surface "$f"; then
            other_modified+=("$f")
        fi
    done < <(git diff --name-only HEAD 2>/dev/null || true)
    other_untracked=()
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        if ! is_release_surface "$f"; then
            other_untracked+=("$f")
        fi
    done < <({ git ls-files --others --exclude-standard; release_note_files; } | sort -u)
    if [[ ${#other_modified[@]} -gt 0 || ${#other_untracked[@]} -gt 0 ]]; then
        die_pre "working tree dirty outside the release surfaces (${#other_modified[@]} modified, ${#other_untracked[@]} untracked); commit or stash first — --abort only reverts dry-run changes"
    fi
    abort_tracked=()
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        if is_release_surface "$f"; then
            abort_tracked+=("$f")
        fi
    done < <(git diff --name-only HEAD 2>/dev/null || true)
    abort_untracked=()
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        abort_untracked+=("$f")
    done < <(release_note_files)
    if [[ ${#abort_tracked[@]} -gt 0 || ${#abort_untracked[@]} -gt 0 ]]; then
        set +e
        if [[ ${#abort_tracked[@]} -gt 0 ]]; then
            git restore --source=HEAD --staged --worktree -- "${abort_tracked[@]}" 2>/dev/null
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

# Resolve the github push remote (v0.113.11: derived by URL, not hardcoded —
# the hardcoded `github` name failed identically on v0.113.9 + v0.113.10
# because this repo names its github remote `origin`). Loud failure when no
# github remote exists; an explicit --remote override is validated instead.
if [[ -n "$REMOTE" ]]; then
    git config --get "remote.${REMOTE}.url" >/dev/null 2>&1 \
        || die_pre "remote '$REMOTE' does not exist (git config remote.$REMOTE.url)"
    ok "push remote: $REMOTE (explicit --remote override)"
else
    REMOTE="$("$SCRIPT_DIR/resolve-github-remote.sh" "$REPO_ROOT")" \
        || die_pre "could not resolve a github remote (see above)"
    ok "push remote: $REMOTE (auto-detected from remote.*.url)"
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    die_pre "version '$VERSION' is not semver (expected e.g. 0.112.12)"
fi

# ----- step 1: test discipline gates (AGENTS.md) -------------------------
log "step 1/${TOTAL_STEPS}: test discipline gates (AGENTS.md)"
# audit LOW 2026-08-10: release.sh used to have NO test/clippy/deny gate —
# the only build check was `cargo publish --dry-run` (compiles, but runs no
# tests), so a single release command could publish a tree that never passed
# the AGENTS.md "Test discipline" gates. Now the four gates run here, before
# any mutation:
#   - they run on the CLEAN pre-bump tree (a failed gate leaves the tree
#     untouched); the version bump below would rewrite the root package's
#     version entry in Cargo.lock, which makes every `--locked` invocation
#     fail with "the lock file needs to be updated", so post-bump gating
#     would need to drop --locked — this is why they run pre-bump.
#   - they always run, even under --dry-run (local, read-only; only
#     target/ is touched).
require_cmd cargo-deny
run_gate() {
    printf '   $ %s\n' "$*"
    "$@"
}
run_gate cargo test --workspace --locked
run_gate cargo build --release --locked
run_gate cargo deny check
run_gate cargo clippy --workspace --locked -- -D warnings
ok "  all gates passed"

# ----- step 2: bump Cargo.toml version ------------------------------------
log "step 2/${TOTAL_STEPS}: bumping ${CRATE_REL}/Cargo.toml to ${VERSION}"
current=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$CRATE_TOML" 2>/dev/null || true)
if [[ -z "$current" ]]; then
    die_pre "no version found in $CRATE_TOML"
fi
if [[ "$current" == "$VERSION" ]]; then
    ok "  $CRATE_TOML already at $VERSION"
else
    # A dry-run is a local preview: write the target manifest version so any
    # subsequent cargo publish --dry-run validation sees the release being
    # previewed, not the old version.
    sed -i "0,/^version[[:space:]]*=/{s/^version[[:space:]]*=.*$/version = \"${VERSION}\"/}" "$CRATE_TOML"
    ok "  $CRATE_TOML: $current → $VERSION"
fi
refresh_workspace_lock

# ----- step 3: close CHANGELOG [Unreleased] -------------------------------
log "step 3/${TOTAL_STEPS}: closing ${CRATE_REL}/CHANGELOG.md [Unreleased] → [${VERSION}]"
DATE=$(date -u +%Y-%m-%d)
# v0.113.11: extracted + idempotent (a re-run on an already-closed
# version leaves the file byte-identical — the v0.113.10 re-run duplicated
# the header).
python3 "$SCRIPT_DIR/close-changelog.py" "$CHANGELOG" "$VERSION" "$DATE"
ok "  $CHANGELOG: [Unreleased] closed as [${VERSION}] - ${DATE} (or already closed)"

# ----- step 4: create release-notes file ----------------------------------
log "step 4/${TOTAL_STEPS}: creating ${CRATE_REL}/release-notes-v${VERSION}.md"
NOTES_REL="$CRATE_REL/release-notes-v${VERSION}.md"
NOTES="$REPO_ROOT/$NOTES_REL"
if [[ -f "$NOTES" ]]; then
    ok "  $NOTES_REL already exists"
else
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
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-utilities/main/dracon-sync/dracon-sync.service \\
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
\`\`\`

**Full Changelog**: https://github.com/DraconDev/dracon-utilities/compare/$(git describe --tags --abbrev=0 2>/dev/null | sed 's/^v//' || echo "0.0.0")...v${VERSION}
EOF
    ok "  $NOTES_REL created"
fi

# ----- step 5: cargo publish --dry-run (sanity) ---------------------------
log "step 5/${TOTAL_STEPS}: cargo publish --dry-run (sanity check)"
# `cargo publish --dry-run` changes no registry state, but it does build the
# exact package directory consumed by the artifact fixture below. Do not send
# it through `run`: skipping this command made a clean --dry-run fail later
# because target/package/<crate>-<version> did not exist.
if ! run_local cargo publish -p "$CRATE_NAME" --dry-run --allow-dirty; then
    die_pub "cargo publish --dry-run failed; fix and re-run"
fi
ok "  dry-run package clean"

# ----- step 6: cargo publish for real -------------------------------------
log "step 6/${TOTAL_STEPS}: cargo publish -p $CRATE_NAME"
# Idempotent re-run path (v0.113.11): when a previous run already published
# this version but failed later (e.g. the v0.113.9/v0.113.10 push step),
# 'already exists on crates.io index' is success, not a fatal error.
confirm_remote_mutation
if [[ $DRY_RUN -eq 1 ]]; then
    run cargo publish -p "$CRATE_NAME" --allow-dirty
else
    printf '   $ cargo publish -p %s --allow-dirty\n' "$CRATE_NAME"
    if ! publish_out="$(cargo publish -p "$CRATE_NAME" --allow-dirty 2>&1)"; then
        if grep -q "already exists on crates.io index" <<<"$publish_out"; then
            ok "  $CRATE_NAME@$VERSION already published; continuing"
        else
            printf '%s\n' "$publish_out" >&2
            die_pub "cargo publish failed — tag NOT created"
        fi
    fi
fi

# ----- step 7: fixture check on the published artifact (2026-08-08 guard) ---
log "step 7/${TOTAL_STEPS}: fixture check on packaged artifact (phantom-untracked guard)"
# The 2026-08-08 incident: `cargo publish` drops [patch.crates-io], so a
# binary installed via `cargo install` resolved an unpatched dracon-git and
# reported gitignored .pi/ files as untracked. Installing from the PACKAGED
# crate reproduces the exact dependency resolution a crates.io install would
# use (no workspace lock, no patch) — if the result fails the fixture, the
# release must not proceed.
PKG_DIR="$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["workspace_root"])' 2>/dev/null || echo "$REPO_ROOT")/target/package/${CRATE_NAME}-${VERSION}"
if [[ -d "$PKG_DIR" ]]; then
    FIXTURE_ROOT="$REPO_ROOT/target/fixture-bin"
    if [[ $DRY_RUN -eq 1 ]]; then
        printf '   $ cargo install --path %s --root %s --force  (skipped: --dry-run)\n' "$PKG_DIR" "$FIXTURE_ROOT"
    else
        run cargo install --path "$PKG_DIR" --root "$FIXTURE_ROOT" --force
        if ! "$SCRIPT_DIR/verify-install.sh" "$FIXTURE_ROOT/bin/dracon-sync"; then
            die_pub "fixture check FAILED on the packaged artifact — release is broken, do NOT tag"
        fi
    fi
else
    die_pub "packaged crate dir $PKG_DIR missing — cannot run fixture check (publish must have failed)"
fi

# ----- step 8: commit, tag, push, gh release ------------------------------
log "step 8/${TOTAL_STEPS}: commit + tag + push + gh release"
# The utility directory is parent-gitignored by design; force staging is
# scoped to the exact release surfaces and never uses `git add .`.
run git add -f -- "$CRATE_REL/Cargo.toml" "$CRATE_REL/CHANGELOG.md" "$NOTES_REL" "Cargo.lock"
# Idempotent re-run path: skip the commit when there is nothing to commit.
if [[ $DRY_RUN -eq 1 ]]; then
    run git -c user.email=dracsharp@gmail.com -c user.name=DraconDev \
        commit --no-verify -m "release: v${VERSION}"
    run git tag "$TAG"
else
    if git diff --cached --quiet; then
        ok "  nothing to commit (release commit already exists)"
    else
        printf '   $ git commit --no-verify -m release: v%s\n' "$VERSION"
        git -c user.email=dracsharp@gmail.com -c user.name=DraconDev \
            commit --no-verify -m "release: v${VERSION}"
    fi
    if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
        ok "  tag $TAG already exists"
    else
        printf '   $ git tag %s\n' "$TAG"
        git tag "$TAG"
    fi
fi
run git push "$REMOTE" main "$TAG"

# Idempotent re-run path: 'gh release create' fails when the release exists.
if [[ $DRY_RUN -eq 1 ]]; then
    run gh release create "$TAG" \
        --target main \
        --title "v${VERSION}" \
        --notes-file "$NOTES"
else
    if gh release view "$TAG" >/dev/null 2>&1; then
        ok "  github release $TAG already exists"
    else
        printf '   $ gh release create %s\n' "$TAG"
        gh release create "$TAG" \
            --target main \
            --title "v${VERSION}" \
            --notes-file "$NOTES"
    fi
fi

# Mirror remotes (codeberg/gitlab) receive main from the daemon's normal
# push cycles, but TAGS are operator-pushed — both v0.113.9 and v0.113.10
# needed manual mirror tag pushes. Remind, with the exact commands.
mirror_remotes=()
while IFS= read -r mline; do
    mkey="${mline%% *}"          # "remote.<name>.url" (strip the value first)
    mname="${mkey#remote.}"; mname="${mname%.url}"
    [[ "$mname" == "$REMOTE" ]] || mirror_remotes+=("$mname")
done < <(git config --get-regexp '^remote\..*\.url$' || true)
if [[ ${#mirror_remotes[@]} -gt 0 ]]; then
    warn ""
    warn "mirror remotes get main from the daemon, but tags are operator-pushed:"
    for m in "${mirror_remotes[@]}"; do
        warn "    git push $m $TAG"
    done
fi

ok ""
ok "════════════════════════════════════════════"
ok "✓ dracon-sync v${VERSION} released"
ok "  crates.io:  https://crates.io/crates/dracon-sync"
ok "  github:     https://github.com/DraconDev/dracon-utilities/releases/tag/${TAG}"
ok "════════════════════════════════════════════"

warn ""
warn "after 'cargo install dracon-sync --version ${VERSION}', run the fixture check:"
warn "    ${CRATE_REL}/scripts/verify-install.sh"

if [[ $DRY_RUN -eq 1 ]]; then
    echo ""
    warn "This was a --dry-run. Local files were modified but no remote state was changed."
    warn "Run '${CRATE_REL}/scripts/release.sh --abort' to revert, or '${CRATE_REL}/scripts/release.sh ${VERSION} --yes' to execute for real."
fi
