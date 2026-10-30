# Report-truth repair + honest fleet state (2026-10-30)

## 1. The auditor was right: `ok` counted outstanding work as clean

`dracon-sync repos` computed its `✅ clean` total as a **residual**:

```rust
let ok = rows.len().saturating_sub(concern + active + warn);
```

Any row holding local work or unpushed commits that did not also happen to be
flagged `active`/`warn` was therefore **silently promoted into the healthy
total**. That is exactly how the previous claim could read `ok=36` while 14 of
36 rows carried dirty trees or unpushed commits. The summary was not a lie about
each row; it was a lie about the aggregate, which is worse because the aggregate
is what a reader trusts.

### The fix

`row_is_converged()` is now the **definition** of clean, and the buckets are
derived FROM it rather than the other way round — deriving "clean" from a
residual lets any future classification change leak work back into the
healthy count:

```rust
pub(crate) fn row_is_converged(row: &RepoReportRow) -> bool {
    row.modified == 0 && row.staged == 0 && row.untracked == 0
        && row.ahead == 0 && row.excluded_dirty == 0
        && row.push_status == "OK"
        && !row.concern && !row.warn && !row.active
        && !row.state_flags.iter().any(|f| f == "DIRTY" || f.starts_with("AHEAD:"))
}
```

A fifth bucket, `pending`, now names the class that used to be rounded into
`ok`: rows with eligible local or unpushed work that are neither a failure nor
converged. `row_bucket_counts` asserts the five buckets partition the rows
exactly once, and the banner reads
`✅ clean · 🟣 pending · 🔄 active · 🟡 warn · ❌ concerns`.

Note  the counters are the daemon's **effective** counts: dirt it intentionally
will not commit is already subtracted upstream, so an intentionally-excluded
file does not make a repo look unconverged.

### Regression coverage

Three tests, each of which fails if `ok` is restored to a residual:

- `test_row_bucket_counts_never_calls_outstanding_work_clean` — seven
  otherwise-converged fixtures, each differing in exactly one field
  (modified / staged / untracked / excluded_dirty / ahead / pending push /
  blocked push). Every one must land in `pending`, never in `ok`.
- `test_row_is_converged_rejects_dirty_and_ahead_flags_alone` — the `DIRTY` and
  `AHEAD:n` **flags** alone disqualify a row, so a report can never disagree
  with its own rows.
- `test_row_buckets_partition_every_row_exactly_once` — exhaustiveness against
  an explicit predicate.

**Negative control** (disposable, then reverted): restoring the old
`let ok = rows.len().saturating_sub(...)` made the first and third tests fail,
with the first failing on precisely the audited defect:

```
modified working tree: must land in `pending`, never in `ok`
  left: (1, 0, 0, 0, 0)      <- ok=1 for a DIRTY row
 right: (0, 0, 0, 0, 1)
```

## 2. The checker now derives from rows, and refuses to trust `ok`

`checks/fleet-observation-2026-10-29/verify-fleet-observation.py` re-derives
convergence from each row's own fields and then **checks the summary against
that derivation**. Disagreement is a hard failure.

Run against the very samples that were previously accepted on `ok`, it
reproduces the auditor's finding exactly:

```
report claims ok=36 but per-row state gives 22
report omits 'pending' (cannot audit it)
14 of 36 rows are NOT converged: browser-extensions-shared [ahead=27,...];
empire [ahead=2,...]; .dracon [modified=2,ahead=1,...]; ... (+6 more)
```

Two disposable controls prove it is not vacuous: a synthesized genuinely
converged fleet PASSES (36/36), and a file whose rows are clean but whose
summary claims `ok=99` FAILS with
`report claims ok=99 but per-row state gives 36`.

## 3. Honest current state — NOT yet converged

With the report fixed, the fleet reads truthfully and it is **not** converged.
Six samples taken after the repair:

```
s1: repos=36 ok=30 pending=0 active=4 warn=2 concern=0 fail=0
s2: repos=36 ok=29 pending=0 active=6 warn=1 concern=0 fail=0
s3: repos=36 ok=30 pending=0 active=5 warn=1 concern=0 fail=0
s4: repos=36 ok=30 pending=1 active=4 warn=1 concern=0 fail=0
s5: repos=36 ok=30 pending=0 active=3 warn=3 concern=0 fail=0
s6: repos=36 ok=31 pending=0 active=4 warn=1 concern=0 fail=0
```

Earlier, before the repair, the same fleet reported `ok=36` with 14 rows
holding outstanding work. The previous acceptance claim was therefore **wrong**,
and this document supersedes it.

### Repairs made during this pass

- **`dracon-platform` was wedged by two abandoned git lock files.** Its commits
  failed with `cannot lock ref 'refs/heads/main'` and then `HEAD.lock`. Both
  locks were 6.3 hours old with **no live process** holding them (`fuser`
  empty, no `git` process for that repo). Removed through the sanctioned
  `dracon-sync maintenance --` wrapper. It then committed 56 files and its
  worktree dropped to 2 entries.
- No hook was bypassed, no policy relaxed, no history rewritten, no manual
  recovery push, and no producer stopped.

## 4. Remaining non-convergence: two genuine, evidenced blockers

**(a) `polis` — at its own repository growth budget.** Its `pre-commit` runs
the bucket high-water guard, which fails closed:

```
"projectedBytes": 2345216557,
"highWaterBytes": 2344615936,
"atOrAboveLimit": true,
"code": "BUCKET_STRATEGY_HIGH_WATER",
"remainingBytes": -600621
```

This is a real, configured limit for polis
(`web/games/wip/polis/web/config/bucket-strategy.json`, 2.18 GiB = its own
measured 1.68 GiB + growth), not a misread of the platform's 5.5 GiB budget.
The repository is 2.77 GB on disk, so it genuinely needs **sanctioned slimming**
(AGENTS.md 2026-09-26) — an operator-approved procedure. Bypassing the guard, or
raising the budget to make the commit pass, are both explicitly forbidden.

**(b) `dracon-strategy` — one file over the hard size limit.** Ordinary staging
expires because 533 untracked files (7.28 GB) including
`market-research/rising-stars/raw/channel-raw.jsonl` at **158.3 MB** exceed
`max_stage_file_bytes = 104857600` (100 MiB). Per AGENTS.md the size limit is a
hard exclusion, so this is not something to work around silently; the operator
must decide whether that artifact belongs in git, belongs in the bucket, or
should be gitignored.

Both are recorded as evidence rather than labelled healthy, and neither was
used to quietly narrow the objective.

## 5. What this pass did NOT do

No detector relaxed, no rule disabled, no file excluded from a gate, no warning
suppressed, no `--no-verify`, no history rewrite, no force-push, no manual
recovery push, no producer stopped, and no hook bypassed.

## 6. Gate receipt and final state

Four gates on one coherent assembly (`final-gates/gates.json`, bound by content
SHAs because the gates compile the working tree, which the daemon may commit
while they run):

| gate | result |
|---|---|
| `cargo test --workspace --locked` | 44 suites, **2923 passed, 0 failed** |
| `cargo build --release --locked` | exit 0 |
| `cargo clippy --workspace --locked --all-targets -- -D warnings` | exit 0 |
| `cargo deny check` | advisories/bans/licenses/sources ok |

All three new regressions are present in that run.

Deployed by the sanctioned shape (atomic replacement, pidfd SIGTERM,
`Restart=always`; `systemctl stop` never used). Loaded, installed and built
binaries are the **same** digest, and `src/` is byte-identical to HEAD.

### Final observation (`observation/s10.json`)

```
36 inspected · ok=30 · pending=0 · active=5 · warn=1 · concern=0 · failures=0
independent re-derivation: 30/36 converged — report and derivation AGREE
```

Still outstanding: `dracon-strategy` (533 untracked / 7.28 GB, 158.3 MB file
over the 100 MiB hard limit), `polis` (at its configured high-water budget),
plus `dracon-platform`, `.dracon`, `ai-auto-writer` and `dracon-sync` in
transient active/dirty states.

So the fleet is **not** converged, and this report does not claim it is. What
changed is that the number now tells the truth: previously the same fleet
reported `ok=36`.

## 7. Incidental damage from my own probing (disclosed and repaired)

While building the installed-execution receipt I ran `sync-now` against
throwaway repositories under `/tmp`. Two side effects occurred, both found and
cleaned:

1. **Six private GitHub repos were auto-created** (`probe2`, `p4`–`p7`,
   `followup`) because the daemon auto-creates remotes named after the
   directory. All six were deleted with the operator's explicit approval.
2. **Nine disposable `/tmp` paths entered the live push-stuck ledger**, which
   then (correctly) blocked their own follow-up work. All nine were cleared with
   `dracon-sync repair stuck-unstuck`.

The receipt now isolates every fixture with a private `DRACON_SYNC_STATE_DIR`
and namespaced directories, and it asserts that nothing leaked into the live
ledger — so this cannot recur. Fixtures no longer touch production state.
