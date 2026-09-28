# dracon-sync v0.113.90 (2026-09-28)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.113.90
- `dracon-sync repos` can complete again. It was blocking on
  `git status --porcelain -z` in warden-hardened repos, waiting on a
  `dracon-warden filter-process` that deadlocks almost immediately; a
  hand-run `git status` in `doomtap` took over 40 s and timed out while
  the same command with the filter bypassed took 0.041 s. Three
  consecutive runs exceeded 10 minutes. The report's dirty classifier
  is now bounded (8 s, cancellable child) and degrades to the existing
  defensive answer on timeout, so real dirt is never hidden. The
  "no clean-filter pass" comment at that call site was wrong and is
  corrected.
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.113.90
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.89...dracon-sync-v0.113.90
