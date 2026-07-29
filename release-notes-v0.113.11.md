# dracon-sync v0.113.11 — tip-keyed guard verdict cache + release.sh hardening

> **Date**: 2026-07-29
> **Source**: pi-goal-list items 1+2 (advisor follow-ups from the
> v0.113.10 review, filed in
> `docs/design/stale-backup-branch-cleanup-2026-07-29.md`).

## 1. Tip-keyed verdict cache for the push-path guard

`github_pack_too_large` runs on every push cycle (it does not use the
report's 1h size cache). Under the v0.113.10 delta semantics the verdict
is fully determined by **(pushed-branch tip, github tracking tips,
limit)** — so v0.113.11 caches it on exactly that key.

- **Cache hits perform NO git subprocess** — the key is resolved by
  reading ref files directly: loose refs, `packed-refs`, the `.git`
  indirection file, `commondir`, and a config-file scan for github
  remotes. The `.git` dir walk is skipped too.
- **A moved tip re-measures** — branch tip, any github tracking tip, or
  the limit. Missing tracking refs are encoded as fresh-remote markers
  so a first fetch also invalidates.
- **Only clean determinations are cached** — the conservative
  detached-HEAD/error fallback is never pinned behind an unmoved key,
  so a transient error can't look permanent. Caller-supplied
  precomputed sizes bypass the cache (the report path has its own).

Why: an actively-committing repo with gitdir ≥ 2 GiB and an over-limit
uncompressed delta (the CAG-wakes-up-pre-rewrite scenario) would
otherwise pay a multi-second `pack-objects` run **every ~65s cycle**.

Tests: 6 new cache tests (per-repo measurement counter proves hits run
no measurement; packed-refs survival; branch-tip / tracking-tip / limit
invalidation; detached-HEAD never cached). **872 daemon tests** +
clippy `-D warnings` green.

## 2. `release.sh` hardening

The script failed identically on v0.113.9 and v0.113.10 (hardcoded
`github` remote name; this repo names it `origin`), and its re-run
after the partial failure duplicated the CHANGELOG header.

- **Remote derivation**: new `scripts/resolve-github-remote.sh` picks
  the github remote by URL (canonical-repo match preferred, sorted-first
  with a warning on ambiguity, loud exit-2 when none). `--remote`
  override still wins and is validated.
- **Idempotent CHANGELOG close**: extracted to
  `scripts/close-changelog.py`; a re-run on an already-closed version
  leaves the file **byte-identical**.
- **Idempotent steps 5/6**: already-published crates.io version,
  existing tag, nothing-to-commit, and existing GitHub release are
  "already done", not fatal errors — a re-run after any partial failure
  now completes the remaining steps.
- **Mirror-tag reminder**: prints the exact
  `git push <mirror> vX.Y.Z` commands for codeberg/gitlab at the end
  (both prior releases forgot them; main reaches the mirrors via the
  daemon, tags don't).

## Upgrade notes

No config changes. The cache is in-memory per daemon process. Test
count: 872 daemon tests (was 866 in v0.113.10).
