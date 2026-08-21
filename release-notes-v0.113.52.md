# dracon-sync v0.113.52

Release date: 2026-08-21

## Fixed

- **Distinct GitHub mirror preserved** (`0fc2a03`): a named `github` mirror
  pointing at a DIFFERENT GitHub repository than `origin` is no longer
  excluded from the mirror path by host-only comparison (live case: doomtap,
  whose `origin` was `github.com/DraconDev/ultratap`). Remote identity is now
  compared transport-neutrally via `canonical_repository_url`
  (`src/git/urls.rs`).
- **Rich `repos` tables no longer wrap pulse counts** (`f97f204`): 1H/6H/24H
  columns reserve five content cells; REM width derives from rendered remote
  labels.
- **Symlink descendants no longer wedge staging** (`e16193b`, `1b466b0`):
  paths below symlink components are skipped before `git add`; direct
  symlink staging is preserved; real files in the same batch still land.
- **Pending pushes are visually distinct from live pushes** (`76f00bd`):
  ACTIVITY shows `🟡 waiting` when there is no fresh in-flight marker instead
  of claiming `🟣 pushing`.

## Deployment

Built with `cargo build --release --locked`, installed to `~/.local/bin`,
daemon restarted via systemd; verified via `dracon-sync --version` and
`dracon-sync health`.
