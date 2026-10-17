#!/usr/bin/env bash
# Pinned bucket deployment — run ONLY via: dracon-sync maintenance -- bash dracon-sync/scripts/deploy-bucket-2026-10-17.sh
# Deploys the qualified 2026-10-17 daemon + runner. No rebuilds, no profile
# field changes except runner.sha256, no credential/IAM changes, no bypasses.
set -euo pipefail

FROZEN_DIR="$HOME/.local/state/dracon-sync/qualified-bucket/20261017-2b74bb7c84cb"
PINNED_SYNC=2b74bb7c84cb9572b35b0ee246bdd29a9e9e56d622fe70fbe46779678d4dfdba
PINNED_RUNNER=352e4de0a452991fa9ed99c9de9afecb633b8399478912a5169c043823893020
OLD_SYNC=b1c9617024345353ab3fd7d5464de4ea68b28390c8e64def2603e92aef39999a
OLD_RUNNER=f1d0ff1fcb8b82d9bb3d835b07393f7cb94dca40c50dfba4136b847240b88612

DEST_SYNC="$HOME/.local/bin/dracon-sync"
DEST_RUNNER="$HOME/.dracon/bin/runner.bundle.mjs"
PROFILE="$HOME/.dracon/state/bucket-runner/profile.json"
ROLLBACK_BASE="$HOME/.local/state/dracon-sync/bucket-rollbacks"

# 0. Frozen source integrity; refuse path-trust.
[[ $(sha256sum "$FROZEN_DIR/dracon-sync" | cut -d' ' -f1) == "$PINNED_SYNC" ]] || { echo 'frozen daemon mismatch'; exit 2; }
[[ $(sha256sum "$FROZEN_DIR/runner.bundle.mjs" | cut -d' ' -f1) == "$PINNED_RUNNER" ]] || { echo 'frozen runner mismatch'; exit 2; }

# Destinations must be regular operator-owned files, never symlinks.
for f in "$DEST_SYNC" "$DEST_RUNNER" "$PROFILE"; do
  [[ -f "$f" && ! -L "$f" ]] || { echo "destination refused: $f"; exit 2; }
  [[ $(stat -c '%u' "$f") == "$(id -u)" ]] || { echo "destination unowned: $f"; exit 2; }
done
# Pre-flight: installed identity must still be the known pre-deployment pair.
[[ $(sha256sum "$DEST_SYNC" | cut -d' ' -f1) == "$OLD_SYNC" ]] || { echo 'installed daemon changed since qualification'; exit 2; }
[[ $(sha256sum "$DEST_RUNNER" | cut -d' ' -f1) == "$OLD_RUNNER" ]] || { echo 'installed runner changed since qualification'; exit 2; }

# 1. Rollback directory outside all watch roots (~/.dracon, ~/Dev).
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
ROLLBACK="$ROLLBACK_BASE/$STAMP"
case "$ROLLBACK" in "$HOME/.dracon/"*|"$HOME/Dev/"*) echo 'rollback inside watch root'; exit 2;; esac
mkdir -p -m 700 "$ROLLBACK"
for f in "$DEST_SYNC" "$DEST_RUNNER" "$PROFILE"; do cp -p -- "$f" "$ROLLBACK/$(basename "$f").rollback"; done
sha256sum "$DEST_SYNC" "$DEST_RUNNER" "$PROFILE" > "$ROLLBACK/pre-deploy.sha256"
chmod 400 "$ROLLBACK"/*.rollback "$ROLLBACK/pre-deploy.sha256"
echo "rollback=$ROLLBACK"

# 2. Atomic sibling-stage + rename for the daemon.
install -m 755 "$FROZEN_DIR/dracon-sync" "$HOME/.local/bin/.dracon-sync.deploy-new"
[[ $(sha256sum "$HOME/.local/bin/.dracon-sync.deploy-new" | cut -d' ' -f1) == "$PINNED_SYNC" ]] || { echo 'staged daemon mismatch'; exit 2; }
mv -f -- "$HOME/.local/bin/.dracon-sync.deploy-new" "$DEST_SYNC"
cmp -s -- "$FROZEN_DIR/dracon-sync" "$DEST_SYNC" || { echo 'daemon install mismatch'; exit 2; }

# 3. Atomic sibling-stage + rename for the runner bundle.
install -m 644 "$FROZEN_DIR/runner.bundle.mjs" "$HOME/.dracon/bin/.runner.bundle.mjs.deploy-new"
[[ $(sha256sum "$HOME/.dracon/bin/.runner.bundle.mjs.deploy-new" | cut -d' ' -f1) == "$PINNED_RUNNER" ]] || { echo 'staged runner mismatch'; exit 2; }
mv -f -- "$HOME/.dracon/bin/.runner.bundle.mjs.deploy-new" "$DEST_RUNNER"
cmp -s -- "$FROZEN_DIR/runner.bundle.mjs" "$DEST_RUNNER" || { echo 'runner install mismatch'; exit 2; }

# 4. Update ONLY profile.runner.sha256 via private fsynced atomic replacement.
python3 - "$PROFILE" "$PINNED_RUNNER" <<'PY'
import json, os, sys
path, pinned = sys.argv[1], sys.argv[2]
with open(path, 'rb') as f: raw = f.read()
profile = json.loads(raw)
assert profile['id'] == 'dracon-platform-private-bridge', 'profile identity drift'
assert profile['runner']['path'] == '/home/dracon/.dracon/bin/runner.bundle.mjs', 'runner path drift'
assert profile['runner']['version'] == 1, 'runner version drift'
profile['runner']['sha256'] = pinned
text = json.dumps(profile, indent=2) + '\n'
tmp = path + '.deploy-new'
fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
try:
    os.write(fd, text.encode())
    os.fsync(fd)
finally:
    os.close(fd)
os.chmod(tmp, 0o600)
os.rename(tmp, path)
PY
python3 - "$PROFILE" "$PINNED_RUNNER" <<'PY'
import json, sys
profile = json.load(open(sys.argv[1]))
assert profile['runner']['sha256'] == sys.argv[2], 'profile sha not pinned'
assert profile['storage']['bucket'] == 'dracon-lfs', 'storage drift'
assert profile['storage']['namespaceRoot'] == 'dracon-platform', 'namespace drift'
assert profile['id'] == 'dracon-platform-private-bridge', 'profile id drift'
PY
chmod 600 "$PROFILE"

# 5. Installed-identity check before restart.
[[ $(sha256sum "$DEST_SYNC" | cut -d' ' -f1) == "$PINNED_SYNC" ]] || { echo 'post-install daemon mismatch'; exit 2; }
[[ $(sha256sum "$DEST_RUNNER" | cut -d' ' -f1) == "$PINNED_RUNNER" ]] || { echo 'post-install runner mismatch'; exit 2; }

# 6. Normal restart (never stop); require active service.
timeout 120 systemctl --user restart dracon-sync.service
timeout 15 systemctl --user is-active dracon-sync.service
pid=$(systemctl --user show dracon-sync.service -p MainPID --value)
[[ "$pid" -gt 0 ]] || { echo 'no daemon MainPID'; exit 2; }
matched=false
for _ in $(seq 1 100); do
  if cmp -s -- "$FROZEN_DIR/dracon-sync" "/proc/$pid/exe"; then matched=true; break; fi
  sleep 0.1
done
[[ "$matched" == true ]] || { echo 'running exe is not the pinned daemon'; exit 2; }
echo "running_pid=$pid"

# 7. Bounded runtime smoke: installed daemon + installed runner + local credential preflight.
timeout 15 "$DEST_SYNC" --version
timeout 15 node "$DEST_RUNNER" --version
timeout 20 node "$HOME/Dev/dracon-utilities/dracon-sync/scripts/check-bucket-credential-access.mjs" --profile "$PROFILE"

# 8. Receipts.
echo "finished=$(date -u +%FT%TZ)"
sha256sum "$FROZEN_DIR/dracon-sync" "$DEST_SYNC" "/proc/$pid/exe"
sha256sum "$FROZEN_DIR/runner.bundle.mjs" "$DEST_RUNNER" "$PROFILE"
systemctl --user show dracon-sync.service -p ActiveState -p MainPID -p ExecMainStartTimestamp | cat
echo "rollback=$ROLLBACK"
