# Combined reliability deployment — 2026-10-15

## What was deployed

The dracon-sync reliability changes committed after the 2026-10-15 15:14
build (running digest `bcedd8b6…`): current-health/report and
scheduler-evidence corrections, status/inspection lifecycle work, and the
rustls 0.23.46 lockfile patch. Same crate version (0.113.95); deployment
identity is by SHA-256 digest, below.

## Gates (all from the workspace root, `--locked`)

- `cargo test --workspace --locked` — 2,412 passed, 0 failed (35 suites).
- `cargo build --release --locked` — exit 0.
- `cargo clippy --workspace --locked -- -D warnings` — exit 0.
- `cargo deny check` — advisories, bans, licenses, sources all ok.

Tested release binary: `target/release/dracon-sync`
SHA-256 `b8621ceefef30f90c5297b73b0a85d3ec0a6329c1b5bdda122d1994f2c90115a`.

## Deployment (sanctioned maintenance, atomic install)

No `systemctl stop` was used. Sequence:

1. Rollback copy preserved before touching the installed binary:
   `~/.dracon/backups/dracon-sync.rollback-20261015-pre-covers-deploy`
   SHA-256 `bcedd8b63a402ba7126020f93148b963be1a92f6eef30057fc119edb26594244`
   (byte-identical to the previously running build).
2. Inside `dracon-sync maintenance -- …` (daemon paused, always resumed):
   `install -m 755 target/release/dracon-sync ~/.local/bin/.dracon-sync.new`,
   atomic `mv` over `~/.local/bin/dracon-sync`, then
   `systemctl --user restart dracon-sync.service`. Exit 0; freeze marker
   removed on completion.

## Post-deploy verification

- Service `active`; new MainPID's `/proc/<pid>/exe` SHA-256 =
  `b8621cee…` — identical to the tested release binary and to the
  installed path.
- `dracon-sync health` — healthy, daemon running, freeze off, policy valid.
- `scripts/verify-install.sh ~/.local/bin/dracon-sync` — OK (gitignore
  handling intact, untracked=0 fixture).
- Journal in the first two minutes after restart shows continued
  commit/sync activity (37 matching lines); no stall.

## Service changes for hook isolation

None were required and none were applied. The push-isolation design
explicitly preserves hook enforcement with no service/limit changes, and
`scripts/check-unit-deployment.sh` exits 0: the deployed user unit
matches the shipped unit, with all ReadWritePaths roots writable in the
running namespace. The pre-existing operator drop-in
`10-resource-headroom.conf` (CPU/memory headroom, 2026-09-28) was left
untouched.

## Rollback steps

If the deployed build misbehaves:

1. `dracon-sync maintenance -- bash -c 'install -m 755 ~/.dracon/backups/dracon-sync.rollback-20261015-pre-covers-deploy ~/.local/bin/.dracon-sync.new && mv -f ~/.local/bin/.dracon-sync.new ~/.local/bin/dracon-sync && systemctl --user restart dracon-sync.service'`
2. Verify: `sha256sum /proc/$(systemctl --user show -p MainPID --value dracon-sync.service)/exe`
   must read `bcedd8b6…`, then `dracon-sync health`.
3. The rollback copy is retained until a later deployment supersedes it;
   deleting it is a manual operator decision, not part of rollback.
