#!/usr/bin/env bash
# Regression test for the release preview's manifest bump and rollback.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work=$(mktemp -d "${TMPDIR:-/tmp}/dracon-sync-release-dry-run-XXXXXX")
trap 'rm -rf "$work"' EXIT
repo="$work/repo"
mkdir -p "$repo/scripts" "$work/bin" "$work/home/.cargo" \
    "$repo/target/package/dracon-sync-0.1.1"

git init -q -b main "$repo"
git -C "$repo" config core.hooksPath /dev/null
git -C "$repo" config user.name fixture
git -C "$repo" config user.email fixture@example.test
git -C "$repo" remote add origin https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote.git

cp "$SCRIPT_DIR/release.sh" "$repo/scripts/release.sh"
cp "$SCRIPT_DIR/resolve-github-remote.sh" "$repo/scripts/resolve-github-remote.sh"
chmod +x "$repo/scripts"/*.sh
cat > "$repo/.gitignore" <<'EOF'
target/
.publish-version
EOF
cat > "$repo/Cargo.toml" <<'EOF'
[package]
name = "dracon-sync"
version = "0.1.0"
edition = "2021"
EOF
cat > "$repo/Cargo.lock" <<'EOF'
version = 4

[[package]]
name = "dracon-sync"
version = "0.1.0"
EOF
cat > "$repo/CHANGELOG.md" <<'EOF'
# Changelog

## [Unreleased]

- fixture

## [0.1.0] - 2026-01-01
EOF

git -C "$repo" add .
git -C "$repo" commit -qm init

touch "$work/home/.cargo/credentials.toml"
cat > "$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
root="${DRACON_FIXTURE_ROOT:?}"
case "${1:-}" in
    test|build|clippy|deny)
        ;;
    metadata)
        printf '{"workspace_root":"%s"}\n' "$root"
        ;;
    publish)
        awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$root/Cargo.toml" > "$root/.publish-version"
        ;;
    *)
        echo "unexpected cargo invocation: $*" >&2
        exit 2
        ;;
esac
EOF
cat > "$work/bin/cargo-deny" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "$work/bin/gh" <<'EOF'
#!/usr/bin/env bash
[[ "${1:-}" == auth && "${2:-}" == status ]]
EOF
chmod +x "$work/bin"/*

DRACON_FIXTURE_ROOT="$repo" HOME="$work/home" PATH="$work/bin:$PATH" \
    timeout 120 "$repo/scripts/release.sh" 0.1.1 --dry-run --yes \
    >"$work/dry-run.out" 2>"$work/dry-run.err"
grep -F 'Cargo.toml: 0.1.0 → 0.1.1' "$work/dry-run.out" >/dev/null
test "$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$repo/Cargo.toml")" = 0.1.1
test ! -e "$repo/.publish-version"
test -z "$(git -C "$repo" tag --list)"

git -C "$repo" diff --quiet -- Cargo.lock
grep -F ' M Cargo.toml' <(git -C "$repo" status --short) >/dev/null

DRACON_FIXTURE_ROOT="$repo" HOME="$work/home" PATH="$work/bin:$PATH" \
    timeout 120 "$repo/scripts/release.sh" 0.1.1 --abort \
    >"$work/abort.out" 2>"$work/abort.err"
test "$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$repo/Cargo.toml")" = 0.1.0
test -z "$(git -C "$repo" status --porcelain)"

echo 'sync release dry-run regression tests: ok'
