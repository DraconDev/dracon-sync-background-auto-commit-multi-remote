# Staged-intent permanent deferral: the daemon's review batch limit gated the user's own staging

## Symptom (objective's named failure class, reproduced)

`junk-runner` had 127 staged files and had not committed since 2026-10-25 20:50
— three days. The journal repeated, every few minutes:

    ⚠️ sync failed (late) for …/junk-runner: captured staged intent exceeds strict batch limit
    🔔 Dispatch Starved (>10 min): dirty but undispatched for 99252s

`dracon-sync repos` reported `staged: 127, modified: 90, untracked: 22,
active: true, state_cause: committing` — i.e. the daemon believed it was
working while it could never make progress.

## Root cause: two different limits were conflated

`max_stage_batch_files = 100` is an OPERATOR DECISION from 2026-06-20 (lowered
from 100000) that keeps a single AUTO-commit reviewable. `SyncPolicy`
documents it as such, and the ordinary staging path honours it correctly by
CHUNKING (`cap_batch_union`): deferred entries stay dirty and are picked up next
cycle.

`staged_epoch::eligible()` applied that same limit to intent the user/agent had
already staged. That is a different question, and there the limit is not a
deferral at all: `materialize_tree` writes the WHOLE captured index, so the
epoch commits all 127 files or none. Nothing the daemon does can make an
existing index shrink — so `ensure!(changed.len() <= stage_batch_limit())` is a
PERMANENT refusal, restated every cycle, forever.

The files were not the problem either: 94 regular files, 8.24 MiB total,
largest 3.6 MiB — all far below `max_stage_file_bytes` (100 MiB).

## Repair (dracon-sync source)

- Added `SyncPolicy::max_captured_staged_files` (default `10_000`, `0` =
  unlimited) and `captured_stage_limit()`.
- `staged_epoch::eligible()` now uses `captured_stage_limit()`, with a comment
  explaining why the review-size limit must not apply to already-captured
  intent. Its stated job is only to keep one tree materialization bounded.
- Per-file guards are unchanged and still apply to captured intent: excluded
  paths and oversized blobs are refused exactly as before, so this admits
  nothing that was previously rejected for a real reason.
- Field registered in `OVERRIDE_COVERAGE_GLOBAL_ONLY` (it is a global-only knob),
  keeping the repo-override triage tripwire honest.

## Regression coverage

New: `captured_staging_above_the_daemon_batch_limit_is_not_permanently_deferred`
— reproduces the real shape (127 small staged files, daemon batch 100), asserts
it commits, and asserts the captured-intent ceiling and the per-file guards
still bite.

Existing test `zero_batch_limit_accepts_all_captured_files_without_relaxing_positive_limit`
asserted the OLD contract (`eligible()` must fail at 120 > 100). That assertion
encoded the bug, so it now pins the new split while keeping every per-file
assertion it already made.

Negative control: restoring `stage_batch_limit()` in `eligible()` makes the new
test FAIL with the exact production error —
`127 already-staged small files must not be refused forever: captured staged
intent exceeds strict batch limit`. The test detects the regression.

## Gates

`cargo test --workspace --locked` — 44 suites, **2893 passed, 0 failed**.
`cargo build --release --locked`, `cargo clippy … -D warnings`,
`cargo deny check` — all exit 0.

Deployed through `dracon-sync maintenance` (pause → install → resume); installed
binary `6a0458d6…` is byte-identical to the qualified release build and the
daemon was restarted onto it (verified via `/proc/<pid>/exe`). Previous binary
backed up under `~/.local/state/dracon/rollback-binaries/sync-20261028T041044Z`.

## What was fixed, and the NEXT distinct blocker

Fixed: the permanent deferral. After deployment junk-runner's journal no longer
says "exceeds strict batch limit"; the epoch now BUILDS the 127-file commit
message and proceeds to the repository's own hooks.

Next blocker, different layer and NOT resolved here: the repository's own
`.githooks/pre-commit` refuses the commit:

    machine-local file is staged: .pi-glla/pending-approval-renders.json
    machine-local file is staged: .pi-glla/session-owner.json
    machine-local file is staged: .pi-glla/update-check.json
    pre-commit: machine-local staging policy blocked this commit.

Those three files are already TRACKED in HEAD (each staged as `M`, not added),
and the hook itself states the durable fix ("Source/evidence permission
requires scoped root rules"). That is a policy decision about what this
repository should track — its hook explicitly says index-only removal is not a
durable fix — so it is left for the repository owner rather than guessed at
here. No hook was bypassed.
