# Dracon Sync

Background, auto-commit, multi-remote — invisible git sync for developer workspaces.

This repository is the **canonical standalone source** for `dracon-sync` on
GitHub, GitLab, and Codeberg. It contains the source code, `Cargo.toml`, tests,
examples, and release metadata.
You can build and install this utility directly from this repo.

## Quick start (standalone build)

```bash
# Clone this repo
git clone https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote.git
cd dracon-sync-background-auto-commit-multi-remote

# Build
cargo build --release

# Install (binary lands in target/release/)
sudo cp target/release/dracon-sync /usr/local/bin/
```

## What is in this repo

- `src/` — utility source code
- `tests/` — integration tests (if present)
- `Cargo.toml` — standalone build manifest with registry dependencies
- `README.md` — this utility's user guide
- `BLUEPRINT.md` — design notes
- `dracon-sync.example.toml` — example config
- `dracon-sync.service` — systemd user-service unit
- `LICENSE`, `SECURITY.md`, `.gitignore`, `.github/` — repo metadata
- `docs/SOURCE_OF_TRUTH.md` — architecture + invariants

## Relationship to the monorepo

| Boundary | Decision |
|----------|----------|
| Source code | This repository's `main` branch |
| Source of truth | This standalone repository |
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
- Service: dracon-sync.service
- Example policy: `dracon-sync/dracon-sync.example.toml`
- Common commands: `dracon-sync status · dracon-sync repos · dracon-sync health · dracon-sync daemon`

## Maintenance

Changes are made in this standalone repository. The `dracon-sync` daemon
watches it and pushes configured remotes; the parent meta workspace does not
mirror source files into it.

## License

AGPL-3.0-only — see [LICENSE](LICENSE).
