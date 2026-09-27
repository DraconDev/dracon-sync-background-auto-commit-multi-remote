# dracon-sync v0.113.88 (2026-09-27)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.113.88
- `dracon-sync pause` now records who paused, not just when: the freeze
  marker gains a `paused by <comm> (pid N): <argv> · tty … · cwd …` line,
  and `dracon-sync repos` names that caller in its freeze notice. Added
  after two unexplained fleet freezes on 2026-09-27 (15:14:59 and
  20:10:47) that left no surviving process, no journal line, and nothing
  in shell history. Line 1 of the marker is unchanged, so existing
  readers are unaffected.
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.113.88
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.87...dracon-sync-v0.113.88
