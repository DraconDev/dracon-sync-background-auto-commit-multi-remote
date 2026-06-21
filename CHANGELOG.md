# Changelog

All notable changes to `dracon-sync` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Note**: prior to 0.112.12, `dracon-sync` was developed inside the
> [`DraconDev/dracon-utilities`](https://github.com/DraconDev/dracon-utilities)
> monorepo. Releases 0.0.0–0.112.11 are recorded in
> [`dracon-utilities/CHANGELOG.md`](https://github.com/DraconDev/dracon-utilities/blob/main/CHANGELOG.md)
> under the `dracon-sync` heading. From 0.112.12 onward, this CHANGELOG
> is the canonical record.

## [Unreleased]

## [0.112.12] - 2026-06-21

### Changed
- **Standalone repo**: `dracon-sync` is now a first-class standalone git
  repository at
  [`DraconDev/dracon-sync-background-auto-commit-multi-remote`](https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote).
  Previously this code lived in
  [`DraconDev/dracon-utilities`](https://github.com/DraconDev/dracon-utilities)
  as a workspace member. Source-of-truth has moved to the standalone repo;
  future releases are cut from there via `scripts/release.sh`.
- **`scripts/release.sh`**: new per-repo release script. Same interface as
  the parent monorepo's `release.sh` (`<version> --yes [--dry-run] [--abort]`),
  scoped to the standalone repo's Cargo.toml, CHANGELOG, crates.io publish,
  and GitHub release. Each utility now releases independently on its own
  cadence.
- **Push-protected remotes**: the verbose repo name
  (`dracon-sync-background-auto-commit-multi-remote`) is the public-facing
  identity. Local directory is `dracon-sync/` for ergonomics. The 4-keyword
  description in the repo metadata ("background, auto-commit, multi-remote")
  is the canonical public description.

### Verified
- `cargo info dracon-sync` confirms version 0.112.12 on crates.io
- `gh release view v0.112.12` (verbose repo) shows the github release
- Daemon's `dracon-sync repos` continues to see this repo and pushes to
  the 3 remotes (github + gitlab + codeberg) on its own cycle

[Unreleased]: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/v0.112.12...HEAD
[0.112.12]: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/releases/tag/v0.112.12
