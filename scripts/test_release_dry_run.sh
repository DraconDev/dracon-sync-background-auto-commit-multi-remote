#!/usr/bin/env bash
# Regression test for the standalone release preview, lockfile update, and rollback.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work=$(mktemp -d "${TMPDIR:-/tmp}/dracon-sync-release-dry-run-XXXXXX")
trap 'rm -rf "$work"' EXIT
repo="$work/repo"
mkdir -p "$repo/scripts" "$work/bin" "$work/home/.cargo" \
    "$repo/target"

git init -q -b main "$repo"
git -C "$repo" config core.hooksPath /dev/null
git -C "$repo" config user.name fixture
git -C "$repo" config user.email fixture@example.test
git -C "$repo" remote add origin https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote.git

cp "$SCRIPT_DIR/release.sh" "$repo/scripts/release.sh"
cp "$SCRIPT_DIR/resolve-github-remote.sh" "$repo/scripts/resolve-github-remote.sh"
cp "$SCRIPT_DIR/close-changelog.py" "$repo/scripts/close-changelog.py"
chmod +x "$repo/scripts"/*
cat > "$repo/.gitignore" <<'EOF'
target/
.publish-version
.publish-dry-run
.publish-real
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
    check)
        version=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$root/Cargo.toml")
        sed -i "/^name = \"dracon-sync\"$/{n;s/^version = .*/version = \"$version\"/;}" "$root/Cargo.lock"
        ;;
    metadata)
        printf '{"workspace_root":"%s"}\n' "$root"
        ;;
    publish)
        version=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$root/Cargo.toml")
        if [[ " $* " == *" --dry-run "* ]]; then
            mkdir -p "$root/target/package/dracon-sync-$version"
            touch "$root/.publish-dry-run"
        else
            touch "$root/.publish-real"
        fi
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
test "$(awk -F'"' '/^name = "dracon-sync"$/{getline; print $2; exit}' "$repo/Cargo.lock")" = 0.1.1
test -e "$repo/.publish-dry-run"
test ! -e "$repo/.publish-real"
test -z "$(git -C "$repo" tag --list)"
if git -C "$repo" diff --quiet -- Cargo.lock; then
    echo 'expected Cargo.lock to be updated' >&2
    exit 1
fi
git -C "$repo" diff --name-only | grep -Fx 'Cargo.toml' >/dev/null
git -C "$repo" diff --name-only | grep -Fx 'CHANGELOG.md' >/dev/null
test -e "$repo/release-notes-v0.1.1.md"

DRACON_FIXTURE_ROOT="$repo" HOME="$work/home" PATH="$work/bin:$PATH" \
    timeout 120 "$repo/scripts/release.sh" --abort \
    >"$work/abort.out" 2>"$work/abort.err"
test "$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$repo/Cargo.toml")" = 0.1.0
test "$(awk -F'"' '/^name = "dracon-sync"$/{getline; print $2; exit}' "$repo/Cargo.lock")" = 0.1.0
test ! -e "$repo/release-notes-v0.1.1.md"
test -z "$(git -C "$repo" status --porcelain)"

echo 'sync release dry-run regression tests: ok'
