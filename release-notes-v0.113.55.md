# dracon-sync v0.113.55 (2026-09-01)

Invisible git sync daemon for deterministic AI-assisted development.

## What's Changed

- Bump version to 0.113.55
- (See CHANGELOG.md for the full list of changes in this release)

## Install

\`\`\`bash
cargo install dracon-sync --version 0.113.55
\`\`\`

## Docker / systemd

\`\`\`bash
# systemd unit (Linux)
curl -fsSL https://raw.githubusercontent.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/main/dracon-sync.service \\
    -o ~/.config/systemd/user/dracon-sync.service
systemctl --user daemon-reload
systemctl --user enable --now dracon-sync.service
\`\`\`

**Full Changelog**: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/v0.113.54...v0.113.55
