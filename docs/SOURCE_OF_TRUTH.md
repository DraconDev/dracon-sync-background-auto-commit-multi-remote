# GitHub / GitLab / Codeberg Feature Façade Repositories (v0.112.7+)

Dracon utility façade repositories are the **canonical "mains"** for each
utility. They make `dracon-sync`, `dracon-system`, and `dracon-warden`
discoverable on GitHub, GitLab, and Codeberg and provide an independently
buildable install target for each.

## Architecture (v0.112.7)

- **Each façade repo contains real source code** (not just navigation
  metadata). The source is mirrored from the `DraconDev/dracon-utilities`
  monorepo's per-utility subdir by `scripts/regenerate_facade_repos.py` on
  every monorepo commit.
- **Each façade repo is independently buildable** with a sibling
  `dracon-libs` repo (and, for `dracon-warden`, a sibling `dracon-utilities`
  repo for the security kit). The `Cargo.toml` uses path deps to the
  siblings.
- **The monorepo is the dev workspace** — it owns the development workflow
  and the source-of-truth content. The 3 façade repos are downstream
  one-way mirrors.
- **Auto-sync** is driven by a monorepo `post-commit` hook that calls
  `scripts/regenerate_facade_repos.py`. The script detects which utility's
  source files changed and regenerates that façade. The `dracon-sync` daemon
  picks up the local change in the façade repo clone and auto-pushes to the
  3 remotes (github, gitlab, codeberg).

## Invariants

1. The monorepo is the source of truth for implementation code, tests,
   release packaging, and changelog entries.
2. Each façade repo mirrors its utility's source code from the monorepo via
   `regenerate_facade_repos.py`. The mirror is one-way (monorepo → façade).
3. Each façade repo's `Cargo.toml` uses path deps to siblings
   (`../dracon-libs` for `dracon-git` / `dracon-system-lib`,
   `../dracon-utilities/dracon-warden/src/security` for the
   `dracon-security` kit).
4. The 3 façade repos are 4-remote aligned (github, gitlab, codeberg, + a
   local clone at `/home/dracon/Dev/facade-repos/` that the daemon watches).

## Why this is not a hack

The 3 façade repos give each utility a discoverable, installable home on
GitHub, GitLab, and Codeberg. The auto-sync mechanism keeps them aligned
with the monorepo's source of truth, so the duplication is mechanical (a
scripted mirror) and never drifts. The alternative — keeping the
implementation only in the monorepo — would mean each utility had no
standalone install target, which is what the operator pushed back on:
"are they mains? we are not pushing to them they are still shells".
