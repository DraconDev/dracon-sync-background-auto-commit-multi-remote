#!/usr/bin/env bash
# From utilities workspace root:
# timeout 2400 nix-shell --pure dracon-sync/scripts/private-bucket-verification.nix \
#   --run 'bash dracon-sync/scripts/verify-private-bucket-gates.sh'
set -euo pipefail
cd "$(dirname "$0")/../.."
evidence=dracon-sync/evidence/private-bucket/audit-repair
mkdir -p "$evidence"
{
  cargo --version
  rustc --version
  node --version
  command -v flock
  pkg-config --modversion openssl
  printf 'OPENSSL_INCLUDE_DIR=%s\nOPENSSL_LIB_DIR=%s\n' "${OPENSSL_INCLUDE_DIR:-}" "${OPENSSL_LIB_DIR:-}"
} > "$evidence/environment.log" 2>&1
run_gate() {
  local name=$1 bound=$2; shift 2
  printf '%s: ' "$name"
  if timeout "$bound" "$@" > "$evidence/$name.log" 2>&1; then
    printf 'PASS\n'; printf 'PASS\n' > "$evidence/$name.status"
  else
    local code=$?
    printf 'FAIL (%s)\n' "$code"; printf 'FAIL %s\n' "$code" > "$evidence/$name.status"
    tail -n 30 "$evidence/$name.log"
    exit "$code"
  fi
}
run_gate integration 180 cargo test -p dracon-sync --locked bucket_assets::tests -- --nocapture
run_gate workspace-tests 1200 cargo test --workspace --locked
run_gate clippy 900 cargo clippy --workspace --locked -- -D warnings
run_gate deny 600 cargo deny check
run_gate release-build 1500 cargo build --release --locked
