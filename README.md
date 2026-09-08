# Dracon Sync

Background, auto-commit, multi-remote — invisible git sync for developer workspaces.

![`dracon-sync status` output](docs/status-output.png)

This page is the standalone guide for `dracon-sync` (also rendered on
crates.io). The canonical source is the `dracon-sync/` directory of the
[`dracon-utilities`](https://github.com/DraconDev/dracon-utilities) monorepo
on `main`; the standalone GitHub/GitLab/Codeberg repos are frozen mirrors.
You can build and install this utility directly from either checkout.

## Quick start (standalone build)

```bash
# Clone this repo
git clone https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote.git
cd dracon-sync-background-auto-commit-multi-remote

# Build (locked: workspace discipline requires --locked)
cargo build --release --locked -p dracon-sync

# Install where the shipped service unit looks (dracon-sync.service
# runs %h/.local/bin/dracon-sync); or run ./install.sh at the
# monorepo root to install all three utilities plus services and hooks.
install -d "$HOME/.local/bin"
install -m 0755 target/release/dracon-sync "$HOME/.local/bin/dracon-sync"
```

## What is in this repo

- `src/` — utility source code
- `tests/` — integration tests
- `Cargo.toml` — standalone build manifest with registry dependencies
- `README.md` — this utility's user guide
- `BLUEPRINT.md` — design notes
- `dracon-sync.example.toml` — example config
- `ai.example.toml`, `providers.example.json` — AI/credential config templates
- `dracon-sync.service` — systemd user-service unit
- `scripts/` — release + install-verification tooling
- `LICENSE`, `SECURITY.md`, `.gitignore`, `.github/` — repo metadata
- `docs/SOURCE_OF_TRUTH.md` — architecture + invariants

## Relationship to the monorepo

| Boundary | Decision |
|----------|----------|
| Source code | The `dracon-sync/` directory of the `dracon-utilities` monorepo (`main` branch) |
| Source of truth | The `dracon-utilities` monorepo; the standalone repos are frozen mirrors |
| Workspace integration | Included by the `dracon-utilities` meta workspace when checked out under `dracon-sync/` |
| Shared libraries | Published `dracon-git` crate from crates.io |
| Operational policy | `~/.dracon/utilities/` TOML files |

## Why this name?

The descriptive name is a deliberate choice for Codeberg/Forgejo, where
descriptive repo names get upvotes and free attention because readers
immediately know what the project does. The full word list (no fillers, no
audience/UX claims) is documented in
[`docs/design/github-feature-repos.md`](https://github.com/DraconDev/dracon-utilities/blob/main/docs/design/github-feature-repos.md).

## Purpose

Watches configured repositories, waits for changes to settle (fingerprint stability / debounce), commits deterministic diff-based messages, and pushes to origin plus configured mirrors. Invisible: runs in the background, no user interaction required.

## Runtime

- Binary: `dracon-sync`
- Service: dracon-sync.service (`systemctl --user enable --now dracon-sync.service`;
  never `systemctl stop` it — use `dracon-sync maintenance -- <cmd>` for git surgery)
- Example policy: `dracon-sync.example.toml` in this repo
  (`dracon-sync/dracon-sync.example.toml` from the `dracon-utilities` monorepo root);
  the live config lives at `~/.dracon/utilities/sync/dracon-sync.toml`
  (`dracon-sync config edit` / `dracon-sync config validate`)
- Common commands: `dracon-sync status · dracon-sync repos · dracon-sync health · dracon-sync daemon`;
  also `sync-now`, `pause`/`resume`/`maintenance`, `once`, `config`, `repair`,
  `ownership`, `scan-bloat` — full list at `dracon-sync --help`

## Maintenance

Changes are made in the `dracon-utilities` monorepo (`dracon-sync/` on `main`).
The standalone repos are frozen mirrors of that tree.

## License

AGPL-3.0-only — see [LICENSE](LICENSE).

---

*Part of the [Dracon](https://dracon.uk) developer workspace.*