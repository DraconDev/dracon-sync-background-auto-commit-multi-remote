# dracon-sync v0.113.87 (2026-09-27)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.113.87
- A frozen daemon is now visible from the rows and from the bottom of
  `dracon-sync repos`, not only from a banner on line 2 of a 59-line
  report. A forgotten `pause` froze 8 repos at `🟣 PENDING` for 32
  minutes on 2026-09-27 and the only symptom in the table was
  `🟡 waiting`; PENDING rows now read `⏸️ frozen Nm` while the daemon is
  frozen, and the notice repeats under the table with the freeze age and
  the watchdog's remaining window.
- (See CHANGELOG.md for the full list of changes in this release)

## Install

```bash
cargo install dracon-sync --version 0.113.87
```

## Docker / systemd

```bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
```

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.86...dracon-sync-v0.113.87
