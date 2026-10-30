# False `PUSH 🩹 BROKEN` from a probe deadline (2026-10-29)

## Symptom

`dracon-sync repos --deep` classified **four healthy repositories as broken**:

```
browser-extensions-shared  flags ['AHEAD:15','BROKEN_HISTORY:probe-failed'] push BROKEN
ai-auto-writer             flags ['OK','BROKEN_HISTORY:probe-failed']       push BROKEN
neonbreak                  flags ['OK','BROKEN_HISTORY:probe-failed']       push BROKEN
darklord                   flags ['OK','BROKEN_HISTORY:probe-failed']       push BROKEN
```

`BROKEN` pushes a row into CONCERN, so the operator was told four repos needed
history repair (clone-from-forge or orphan cutover) when none did.

## Evidence that the histories are sound

Direct read-only probes on the same trees, same git:

| repo | `rev-list --objects HEAD` | objects |
|---|---|---|
| neonbreak | exit 0, 6.2 s | 12,215 loose + 80,131 packed |
| darklord | exit 0, 4.7 s | 9,427 loose + 92,428 packed |
| ai-auto-writer | exit 0, 8.2 s | 777 loose + 687,075 packed |
| browser-extensions-shared | exit 0, 3.6 s | 1,719 loose + 472,359 packed |

Also `git fsck --connectivity-only` reported only dangling blobs (normal), and
`git rev-parse --is-shallow-repository` is `false`. Nothing was missing.

## Root cause

`probe_history` ran two bounded steps (`rev-list`, then `cat-file
--batch-check`) with a **10 s** deadline and one retry, and collapsed every
non-success into a single `failed: true`:

```rust
let Some(list) = bounded_with_retry(&["rev-list", "--objects", "HEAD"], &[]) else {
    return HistoryProbe { missing_objects: 0, failed: true };
};
```

`history_broken = history_probe_failed || missing_objects > 0`, so a **deadline
expiry** was rendered as evidence of damage. The function's own comment already
said the opposite was intended ("a timeout is NOT evidence of damage"), but the
type could not express it — `run_git_bounded` returned `Option`, discarding the
distinction between "git said no" and "we gave up waiting".

Measured under real load (host load 30-95 all evening, 30 concurrent agent
sessions editing watched repos): `rev-list` alone took 3.6-8.2 s, so two such
steps plus retry latency routinely exceeded the bound for the larger object
databases — exactly the repos that look most alarming when wrongly flagged.

## Repair

1. `run_git_bounded` now returns an explicit three-way outcome:

```rust
enum BoundedProbe { Output(Vec<u8>), Failed, TimedOut }
```

   `Failed` = git itself rejected the request (non-zero exit) — real evidence.
   `TimedOut` = the deadline expired — no evidence either way.

2. `HistoryProbe` carries `timed_out` separately from `failed`. The retry helper
   returns `Failed` immediately (a second attempt cannot change a rejection) and
   reports `TimedOut` only after both attempts time out.

3. `history_broken` stays exactly `failed || missing_objects > 0`, so only
   git's own rejection or positively counted missing objects can mark a row
   BROKEN. An unmeasured history gets a distinct, honest flag:

```
HISTORY_PROBE_TIMED_OUT
```

   — visibly not-healthy-unknown, without claiming damage.

4. The hint/status wording that said "invalid HEAD/ref or timeout" now says
   "rejected by git — invalid HEAD/ref", because the timeout case can no longer
   reach that arm.

5. **Cache correctness.** The size/probe cache is honoured for
   `REPO_SIZE_CACHE_TTL_SECS` (1 hour). Entries written by the pre-repair binary
   store `history_probe_failed: true` with no `history_probe_timed_out` key, so
   honouring them re-served `PUSH 🩹 BROKEN` for the full hour after deployment
   — observed live. The completeness predicate was extracted into
   `cache_entry_is_usable()` and now additionally requires
   `history_probe_timed_out.is_some()`, so a pre-split entry is rejected and
   costs exactly one recompute.

## Verification

- `report::tests::probe_timeout_is_unmeasured_not_broken_history` — drives the
  real bounded runner with a zero deadline and asserts `TimedOut` (not
  `Failed`), then asserts the BROKEN predicate stays false for that outcome.
- `report::tests::pre_split_cache_entries_are_not_honoured` — loads a real
  legacy cache JSON (no `history_probe_timed_out`) and asserts the **production**
  `cache_entry_is_usable()` rejects it, still accepts a current entry for both
  `timed_out` values, and still enforces the TTL.
- `report::tests::probe_history_retry_still_reports_genuinely_invalid_head_as_failed`
  (pre-existing) — a genuinely invalid HEAD still reports `failed`, so the split
  cannot mask real damage.

### Disposable negative controls

| control | result |
|---|---|
| collapse `TimedOut` back into `Failed` (pre-fix semantics) | `probe_timeout_is_unmeasured_not_broken_history` **FAILED** |
| remove `&& c.history_probe_timed_out.is_some()` from the **production** predicate | `pre_split_cache_entries_are_not_honoured` **FAILED** |

The second control is why the cache test was rewritten: its first version used a
local closure copying the predicate, and it **passed** against the broken
production code — a test that would have guarded nothing.

## Live result after deployment

```
before: repos 36 · ok 28 · active 3 · warn 2 · concern 4 · failures 0
after : repos 36 · ok 31 · active 3 · warn 2 · concern 0 · failures 0
        BROKEN rows: 0
        HISTORY_PROBE_TIMED_OUT rows: browser-extensions-shared, ai-auto-writer
```

The two remaining unmeasured rows are the largest object databases in the fleet
(687k and 472k packed objects) being probed on a host at load ~50; they are
reported as unmeasured, which is the truth.

## Scope honesty

This fixes **reporting accuracy**, not history. It does not make a slow probe
fast, does not raise the 10 s bound (deliberately — the bound is what keeps a
cold 36-repo render from stalling), and does not assert that any repository's
history is undamaged beyond what the successful probes measured. Gates for the
two deployed revisions: 2,918 and 2,919 workspace tests passed / 0 failed plus
release build, strict all-target Clippy and `cargo deny`.
