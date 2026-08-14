# Dracon Sync Source of Truth

`dracon-sync` is a standalone repository and the canonical source for its
implementation, tests, configuration examples, and release metadata.

The parent `dracon-utilities` repository is a meta workspace. When this repo
is checked out at `dracon-utilities/dracon-sync/`, the parent Cargo workspace
includes it by path; the parent does not mirror or overwrite its files.

The daemon watches this repository directly and pushes its configured remotes.
Releases are cut from `scripts/release.sh` in this repository. The git helper
dependency is the published `dracon-git` crate from crates.io; no sibling
`dracon-libs` checkout is required.

## Invariants

1. `main` is the active development branch.
2. The working tree remains buildable with `cargo test --workspace --locked`
   when used from the parent workspace or with `cargo test --locked` here.
3. The daemon's history rules in the parent `AGENTS.md` apply to this repo:
   agent loops do not rewrite published history.
