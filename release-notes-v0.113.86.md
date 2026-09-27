# dracon-sync v0.113.86 (2026-09-27)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.113.86
- `dracon-sync repos`: the SIZE column measures each repo's `.git`
  again in the default (non-`--deep`) view. It has read `?` for every
  repo since v0.113.55, which deferred the whole cold-path compute
  instead of only the expensive probes. Only `du -sb` over submodule
  gitdirs, the GitHub pack-size guard, and the broken-history probe
  stay behind `--deep`; a superproject with unmeasured submodule
  gitdirs now renders `own+?` rather than silently dropping the
  suffix. Measured cost on the live 34-repo fleet: 3.6s → 4.2s.
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.113.86
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.85...dracon-sync-v0.113.86
