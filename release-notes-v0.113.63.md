# dracon-sync v0.113.63 (2026-09-17)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.113.63
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.113.63
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-utilities/main/dracon-sync/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-utilities/compare/dracon-sync-v0.113.62...v0.113.63
