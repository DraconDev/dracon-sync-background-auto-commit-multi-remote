# dracon-sync v0.112.13 (2026-06-21)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.112.13
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.112.13
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/1.0.0...v0.112.13
