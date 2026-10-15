# Repository health: history versus current convergence

## Two independent axes

`dracon-sync repos` now separates **PUSH** (the last observed daemon push
result and its observation age) from **STATUS / ACTIVITY** (current convergence).
A green historical push never means later edits are safely published. Conversely,
an old failed push does not make an unchanged, currently converged repo unhealthy.

`repos --legend` and the width-appropriate default legend explain the same rules.
The shared classifier feeds vertical, compact, full, rich and summary output.
JSON adds a typed `health` object; its `current` value is the same classifier used
by human output. Existing raw `push_status` remains legacy dispatch metadata,
NOT a last-result API. Existing `last_push` remains upstream commit age for
compatibility; it was never a reliable push timestamp. The full table calls that
field **REF AGE**, and detail calls it **upstream commit age**.

## Current legend

| Label | Evidence and meaning |
| --- | --- |
| CLEAN / idle | No eligible modified/staged/untracked files and no observed ahead/behind work. Old commits, old errors, and no scheduler record do not make this unhealthy |
| EXCLUDED | Only policy-excluded dirt remains, ownership prevents daemon handling, or publication is explicitly isolated. These bytes are not claimed covered |
| QUEUED | A reserved worker has not acquired local capacity, an inspection/provisioning handle lacks an execution-start stamp, or current filter-aware eligible work is awaiting admission without starvation evidence |
| WORKING | A fresh scheduler observation shows a started, still-owned sync worker. This is started ownership evidence, not a per-operation progress heartbeat or proof of publication |
| RETRYING | Owned cancellation is awaiting join, or a live scheduler retry/cooldown gate delays another attempt |
| BLOCKED | A current guard/manual-intervention outcome, damaged history, pack guard or pull blocker prevents convergence |
| STALLED | Current unpushed work exhausted its push budget, or continuously observed eligible dirt has neither a current worker/gate nor admission for at least 15 minutes |
| PAUSED | Outstanding work while the daemon is frozen. Unchanged idle rows remain clean |
| UNKNOWN | Outstanding work exists but fresh affirmative scheduler evidence is missing. Neither an old commit nor a stale snapshot proves a stall or active execution |

Excluded counts remain visible alongside mixed eligible work. A stale historical
warning cannot override a fresh started worker; a completed old push cannot
override queued new edits. Only explicit `PUSH_STUCK` budget exhaustion proves
budget-based stalling; legacy `STUCK`/`FAIL` may be a single retryable failure.
Likewise legacy `STUCK_PULL` means behind-with-upstream, not a proven pull blocker;
ordinary behind work follows actual scheduler evidence or remains unknown. No operation heartbeat is invented, and reserve age
is never called execution age or last progress.

"Clean" is a local/current-upstream observation, NOT an all-mirror verification,
bucket-durability guarantee, secret scan or proof that every storage gate will
accept future edits. Deep checks can remain explicitly deferred as before.

## Scheduler observations

`daemon/scheduler_evidence.rs` writes a separate version-1 snapshot beside existing
state files. Legacy in-flight and dispatch-hold files keep their existing writer
formats; sticky old hold strings are not treated as authoritative current gates.
The scheduler copies its owned maps once per completed cycle:

- `SyncRecovery` distinguishes unset execution stamp (queued), acquired-and-started
  worker (working), cancellation still owned (cancelling), and joined-error retry.
  Observation does not release a reservation, abort a task or change admission.
- Actual status/classification reservations establish inspecting metadata, but
  render queued because pending handles cannot prove execution started. The same
  conservative rule applies to provisioning jobs. Actual retry deadlines establish retrying. A known blocked outcome plus a cooldown remains
  blocked, not a fabricated transient outage.
- `dirty_secs` is the session's first continuously dirty observation, preserved
  across fingerprint churn, not the age of the last commit or an asset mtime.
- `eligible` is set only after effective filter-aware classification this cycle;
  provisional raw dirt cannot establish starvation.
- `admitted_secs` is time since reservation, not execution or successful progress.
- Pause, initial/configuration-change discovery, and orderly shutdown publish
  explicit modes. Empty valid snapshots are distinct from unavailable evidence.

Consumers load one snapshot per report, reject missing/corrupt/unsupported,
future-dated and older-than-30-second evidence, and never interpret an absent
repository entry as an exclusion. A daemon crash can leave observations within
that bounded freshness window; output describes the last observation, not an
independent process heartbeat. Repos outside discovery policy are not added to
reports merely to manufacture an excluded row.

## Historical push observations

`daemon/push_history.rs` records only actual daemon-recorded push success/failure
callbacks, not ordinary clean sync cycles or remote commit dates. A transient
push failure is still a failed observation even though it does not consume the
stuck budget. Interrupted attempts do not manufacture a result. Records are
per-repo, versioned and atomically replaced, so updates for different repos do
not race a fleet-wide read/modify/write map. They store no error or credentials.

Before the updated daemon records an attempt, PUSH is **unknown**. Old Git
history is not retroactively fabricated into success evidence. Future/corrupt or
wrong-repository records also render unknown. New commits or edits do not alter
the older observed result; current convergence remains independently evaluated.
No binary is installed or daemon restarted by this implementation item.

## Verification map (implementation checkpoint)

- `current_health_tests.rs`: table-driven states and actual status/label/push/JSON
  outputs, crossed with both historical success and failure and ancient commits.
  Includes queued, started, inspecting, retrying, cancelling, excluded, starved,
  recently admitted, fresh edits without observations, exhausted budget and idle.
- `scheduler_evidence.rs` tests: freshness boundaries, future/unsupported/malformed
  snapshots, atomic empty-snapshot roundtrip, nonmutating reservation/start/cancel/
  retry observations, provisional confidence and gate precedence.
- `push_history.rs` test: observed history survives later edits; version/future/
  malformed records are refused and failure supersedes success.
- Existing report width tests retain single-line rows at 165/200/320 columns;
  fixtures claiming stalls now supply continuous-work evidence instead of old
  commit timestamps. Summary count and legend assertions use the current labels.

Raw qualification evidence: `evidence/current-health-2026-10-15/`. Initial report
qualification found outdated expected labels and one natural-hint-ending assertion
that assumed an author suffix already removed in older code. The fixture now
checks the actual hint's natural ending rather than an unrelated author name.
Final post-patch qualification:

| Gate | Result | Evidence |
| --- | --- | --- |
| `cargo test --workspace --locked --jobs 1 -- --nocapture` | 2,412 passed, 0 failed, 25 existing ignored across 35 summaries | `workspace-final.log` |
| `cargo clippy --workspace --locked --jobs 1 -- -D warnings` | passed | `clippy-final.log` |
| `cargo build --release --locked --jobs 1` | passed | `release-final.log` |
| `cargo deny check` | advisories, bans, licenses, sources all passed | `deny-final.log` |
| `target/release/dracon-sync repos --legend` | exited 0; shipped CLI legend captured | `legend.txt` |

These paths are relative to the evidence directory above. The initial independent
source review (`review-initial.md`) found two P1 classification issues: retryable
push failure could falsely establish a stall, and ordinary behind work could
falsely establish a pull blocker. Both are corrected as described above. Added
output fixtures cover legacy STUCK/FAIL without scheduler evidence, behind work
across absent/working/queued/retrying/blocked observations, and retention of hard
pack guards even over started work. The final logs above were rerun after both
corrections. Follow-up independent review remains pending; this is not completion
or live deployment evidence.
No service restart, binary installation or fleet fault injection occurred.

The first dependency audit found four rustls advisories (RUSTSEC-2026-0346 through
0349) and a yanked yoke-derive release in the pre-existing workspace lockfile.
Only the parent workspace lockfile was patched: rustls 0.23.45 → 0.23.46 and
yoke-derive 0.8.3 → 0.8.4. No dependency policy exception was added. The subsequent
`cargo deny check` passes all four categories; final build/test qualification uses
these patch versions. Existing standalone utility lockfiles are not rewritten by
this workspace-qualified item.
