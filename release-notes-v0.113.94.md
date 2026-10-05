# dracon-sync v0.113.94 (2026-10-05)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Stale `index.lock` janitor runs every pulse: mid-session locks
  (crashed git, killed stage task) are cleared after 120s when
  `fuser` confirms no holder, instead of stalling the repo until
  the next restart (dracon-platform stalled 6h twice on 2026-10-05).
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.113.94
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.93...dracon-sync-v0.113.94
