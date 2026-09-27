# dracon-sync v0.113.89 (2026-09-27)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.113.89
- The filter-aware diff budget now scales with the work instead of being
  a flat 30s. On a warden-managed repo each `filter=dracon` file costs an
  `age` decryption subprocess, so `dracon-platform` (65 modified, 25 of
  them encrypted) measured 67.7s against that 30s ceiling — 2.3x over —
  failed classification every cycle, was never dispatched, and kept 29
  commits off every forge. The budget is now `30s + 3s per modified
  file`, clamped at 180s, and the pending watchdog is derived from it
  (3x) so the bounds cannot drift apart again. Small repos keep the
  exact 30s they had.
- Both the status and classification pipelines are now capped on work
  in flight (4 and 2). They were bounded only per repo, so a fully dirty
  fleet opened ~34 index walks and N filter-aware diffs at once and
  starved each other — the whole `dracon-platform` family logged
  `status inspection wedged over 60s` in the same second while a
  hand-run `git status` on those repos took under a second.
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.113.89
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.88...dracon-sync-v0.113.89
