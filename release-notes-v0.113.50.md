# dracon-sync v0.113.50 — 2026-08-09

## Classify push failures in alerts and the stuck ledger

**Origin**: the `pi-goal-loop-audit` history-divergence incident
(2026-08-09). The loop agent's `git reset` forked the mirrors (59
commits on gitlab/codeberg that github/local don't have); the daemon
correctly refused to force-push and hit the stuck budget. But the
operator-facing text misdirected: the "Mirror Degraded" alert said
**"mirror may be unreachable"** — implying a network/credentials
problem — when the real cause was a **history fork** (non-fast-forward
rejection). The stuck-ledger `last_error` also lost the rejection
reason (only the failing remote names were recorded).

**Fix**:

- New `classify_push_failure()` (`src/git/push.rs`) maps a raw push
  error to one of four operator-actionable causes:
  - `history divergence (non-fast-forward: remote has commits not on
    local; needs operator reconciliation)`
  - `server-side policy rejection (protected branch / hook declined /
    missing repo / lost key)`
  - `pack exceeds forge size limit (needs history rewrite)`
  - `transport/auth failure (network, timeout, or credentials)`
- The per-remote failure tracking (`remote_failures` /
  `mirror_consecutive_fails`) now carries `RemoteFailInfo { consecutive,
  last_error }` instead of a bare count, so the raw error survives to
  the reporting layer.
- The "Mirror Degraded" alert names the classified cause instead of
  "may be unreachable".
- The stuck-ledger `last_error` appends a deduplicated cause line, so
  the `repos` HINT column shows **WHY** (e.g. `history divergence`)
  not just **WHO** (which remotes).

**Tests**: 1244 passed / 9 ignored (was 1241; +3: classifier coverage
for all four modes, dedupe across two divergent mirrors, empty-map
fallback; the mirror-failure tracking test now also asserts the raw
error is captured). Clippy `-D warnings` clean; `cargo deny check`
clean.

**Docs**: `docs/design/pi-goal-loop-audit-divergence-2026-08-09.md`
(incident analysis + reconciliation options).
