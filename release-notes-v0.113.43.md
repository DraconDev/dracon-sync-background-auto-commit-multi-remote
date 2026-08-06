# dracon-sync v0.113.43 — refresh-visibility: prefer `github` remote over `origin`

**Release date**: 2026-08-06
**Severity**: bug fix (cache poisoning, visible UX bug)
**Operator action**: none required (auto-install via system update).
After upgrading, run `dracon-sync refresh-visibility` once if you
want to re-verify the cache (the daemon will refresh within 24h on
its own).

## Summary

Fixes a visibility-cache poisoning bug where `dracon-sync
refresh-visibility` would query the wrong GitHub repo when the
local repo's `origin` remote was mispointed. The `origin` remote was
tried first, and for at least one fleet repo
(`folder-auto-banner-fab`) `origin` pointed at a *different*,
intentionally-public repo (`DraconDev/folder-auto-banner`), so the
refresh cached `public` for the local repo (which is actually
private).

This caused two user-visible issues:

1. `dracon-sync repos` rendered nearly every row with a blank
   visibility column (no 🔒), because the poisoned cache value
   disagreed with the GitHub truth and the 24h TTL meant it took a
   day to self-correct.
2. The visibility cache poisoned by the refresh two weeks earlier
   contributed to the "are these public?" panic response and made
   the actual drift (two GitLab mirrors genuinely public) harder
   to spot.

## Changes

### Bug fix

- `refresh-visibility` now prefers the named `github` remote over
  the legacy `origin` remote. The `github` remote is the canonical
  name the daemon uses for its multi-remote push path, so it
  matches the operator's expected behavior. The `origin` fallback
  is preserved for repos that only have `origin` (the common case).
- The remote-selection logic is extracted into a new helper
  `visibility::select_github_remote_url` so the preference order is
  unit-testable without spawning `git`.

### Tests

Four new unit tests cover `select_github_remote_url`:

- `test_select_github_remote_prefers_github_over_origin` — the
  regression test for `folder-auto-banner-fab`: both remotes exist,
  origin is mispointed, the helper picks the correct GitHub repo.
- `test_select_github_remote_falls_back_to_origin` — the common
  case: repo only has `origin`.
- `test_select_github_remote_skips_empty_github_uses_origin` —
  defensive: if `github` exists but is empty, fall back to `origin`.
- `test_select_github_remote_returns_none_when_nothing` — no
  remotes present.

### Documentation

- `docs/design/refresh-visibility-origin-preference-2026-08-06.md`:
  full incident report, root cause, fix, and lessons learned.

## What does NOT change

- **The 24h cache TTL is unchanged.** Stale → unknown → blank is
  still the documented behavior.
- **GitLab visibility drift detection is still absent.** The daemon
  reads GitHub visibility as the source of truth and does not
  proactively audit GitLab mirror visibility. A future
  `dracon-sync audit-gitlab-visibility` subcommand would address
  this — tracked as a follow-up, not in scope for v0.113.43.
- **Multi-remote push behavior is unchanged.** The daemon still
  pushes to `github` and `gitlab` remotes named in the policy.

## Verification

- All 1232 unit tests pass (4 new).
- `cargo clippy --workspace --locked -- -D warnings` clean.
- `cargo deny check` clean.
- `cargo build --release --locked` clean.
- After install + restart, `dracon-sync refresh-visibility` queries
  the correct GitHub repo for `folder-auto-banner-fab` (caches
  `private`).
- `dracon-sync repos` renders 🔒 for all private repos and blank
  for the intentionally-public ones.

## Operator action items

- **None required for the fix itself.** The daemon will refresh
  visibility within 24h on its own. To verify immediately, run
  `dracon-sync refresh-visibility` after upgrading.
- **Recommended (already done for this fleet)**: if you have any
  repos with a mispointed `origin` remote, fix it:
  ```bash
  git -C /path/to/repo remote set-url origin <correct-github-url>
  ```
  The daemon's multi-remote push path already uses the `github`
  remote, so `origin` is only consulted by `refresh-visibility`.
  But a mispointed origin can also break VSCode's upstream
  tracking.
