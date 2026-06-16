# Dracon Sync

Background, auto-commit, multi-remote — invisible git sync for developer workspaces.

This repository is the **canonical "main"** for `dracon-sync` on GitHub,
GitLab, and Codeberg. It contains the actual source code (mirrored from the
[`DraconDev/dracon-utilities`](https://github.com/DraconDev/dracon-utilities)
monorepo), the `Cargo.toml`, tests, examples, and the per-utility README.
You can build and install this utility directly from this repo.

## Quick start (standalone build)

```bash
# Clone this repo
git clone https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote.git
cd dracon-sync-background-auto-commit-multi-remote

# Clone required siblings (path-dep layout)
git clone https://github.com/DraconDev/dracon-libs.git ../dracon-libs



# Build
cargo build --release

# Install (binary lands in target/release/)
sudo cp target/release/dracon-sync /usr/local/bin/
```

## What is in this repo

- `src/` — utility source code
- `tests/` — integration tests (if present)
- `Cargo.toml` — standalone build manifest with path-dep siblings
- `README.md` — this file (the per-utility README from the monorepo is at `monorepo-README.md`)
- `BLUEPRINT.md` — design notes
- `dracon-sync.example.toml` — example config
- `dracon-sync.service` — systemd user-service unit
- `LICENSE`, `SECURITY.md`, `.gitignore`, `.github/` — repo metadata
- `docs/SOURCE_OF_TRUTH.md` — architecture + invariants

## Relationship to the monorepo

| Boundary | Decision |
|----------|----------|
| Source code | Mirrored from `dracon-utilities/dracon-sync` via `scripts/regenerate_facade_repos.py` on every monorepo commit |
| Source of truth | `dracon-utilities` monorepo (the auto-sync is one-way) |
| Feature surface | This repo (canonical main for `dracon-sync`) |
| Shared libraries | Sibling `dracon-libs` workspace (`../dracon-libs`) |
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
- Service: dracon-sync.service
- Example policy: `dracon-sync/dracon-sync.example.toml`
- Common commands: `dracon-sync status · dracon-sync repos · dracon-sync health · dracon-sync daemon`

## Maintenance

When the monorepo changes the utility source code, README, or example config,
the monorepo's `post-commit` hook calls `scripts/regenerate_facade_repos.py`
which mirrors the changes to this repo. The `dracon-sync` daemon picks up
the local change in `/home/dracon/Dev/facade-repos/dracon-sync-background-auto-commit-multi-remote` and
auto-pushes to the 3 remotes (github, gitlab, codeberg). No manual
`--apply` or `--push-all-remotes` invocation is needed in the normal flow.

## License

AGPL-3.0-only — see [LICENSE](LICENSE).
