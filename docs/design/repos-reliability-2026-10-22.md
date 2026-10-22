# Repos inspection and truthful recovery reporting — 2026-10-22

## Scope and authorization

Investigate the supplied fleet table, fix daemon/report defects, deploy the tested
build, and observe ordinary autonomous sync. The operator explicitly permitted
normal daemon commits/pushes after deployment, but **not** `sync-now`, applied
repairs, manual pushes, or repository-state repairs. No history rewrite,
force-push, deletion, guard bypass, target removal, or coverage reduction is part
of this work. The masked user service is obsolete; the actual daemon is the
system-level `dracon-sync-operator.service`.

## Diagnosis

### Status timeouts: report deadline, not proof of a hung daemon

The installed CLI reproduced failures for ai-auto-writer, platform, and
junk-runner. Plain read-only owned-equivalent Git status subsequently succeeded:

```
ai-auto-writer: exit 0, 10.81 seconds, 552 output bytes
platform:      exit 0, 12.67 seconds, 618 output bytes
junk-runner:   exit 0,  4.83 seconds, 121 output bytes
```

Git Trace2 on platform showed 1.196s index refresh, 6.849s nested-worktree
inspection, and 0.802s untracked enumeration (8.882s total). This is successful
work exceeding the report's hard-coded eight-second deadline, not evidence of
an infinite filter hang. Scheduler inspection already allows sixty seconds.
`--no-optional-locks`, filters, thread limits, untracked enumeration, and owned
process-group cancellation remain intact. Reports now share the scheduler's
finite inspection budget and admit four concurrent repositories instead of
sixteen competing full scans. Worst-case fleet latency still scales with the
number of batches; this is not a new constant-time guarantee.

Detail requests previously inspected the entire fleet, emitted unrelated errors,
then discarded failed rows and incorrectly said the requested repo was absent.
They now inspect only exact-basename candidates (retaining ambiguity detection),
and propagate known inspection failures as errors rather than “not found”.

### aud-vis: two directories reference one worktree

Read-only inspection found both `.git` files contain:

```
gitdir: ../../../../.git/modules/web-games-capture-anime-girls
```

That Git directory has `core.worktree = ../../../web/games/wip/capture-anime-girls`.
`git rev-parse --show-toplevel` from **both** paths returns capture-anime-girls.
Thus aud-vis is not an independent Git checkout: Git operates on the other
worktree, and both paths compete for its operation lease. Neither directory was
removed or excluded. Correcting the checkout wiring requires separate operator
approval and content-preserving comparison; silently deduplicating it would
hide possible edits in the alias directory. Inspection now validates Git's
resolved `--show-toplevel` against the canonical requested directory under the
same owned deadline. A mismatched checkout fails visibly before scheduling
neighbor work. Ordinary symlinks to the same worktree remain valid. This does
not repair the alias or infer what its files ought to contain.

There was also a daemon defect: lease contention returned `PushPaused`, falsely
logging “committed but push paused”. The distinct `OperationDeferred` result now
records no commit/push claim, retains failure and remote-backoff evidence, and
sets a short staging cooldown to avoid repeated contention. UNKNOWN's generic
hint no longer promises convergence without scheduler evidence. UNKNOWN still
means unknown; it is not reclassified healthy to make the table green.

### doomtap: bucket-policy guard, not network failure

Journal at 2026-10-22 17:33:37–38 BST captured both origin and GitLab refusing:

```
"code": "BUCKET_STRATEGY_INVALID_POLICY"
"message": "invalid bucket strategy policy"
"buckets: expected non-empty object"
pre-push: bucket high-water/forward-only guard blocked this push
```

The stuck ledger records five consecutive failures. Both remote branch queries
succeeded at 20:14 UTC; reachability does not establish push eligibility.
**Unapplied recommendation:** inspect the bucket policy consumed by the repo's
pre-push hook and `web/scripts/bucket-strategy.mjs` in dracon-platform, restore a
valid nonempty bucket mapping without removing the guard, validate with that
script's read-only policy checks, then authorize clearing doomtap's exhausted
retry budget. No bucket policy or retry ledger was changed here. By deployment time doomtap
had independently recovered: the first post-deployment snapshot shows no
pending changes/ahead commits and a successful recorded push. That recovery
is not attributed to this utility patch; final observations below retain its
current disposition.

### dracon-strategy: exhausted GitLab idle timeout

Journal at 2026-10-22 14:26:58 BST:

```
push to gitlab failed ... after 900s idle (3600s hard cap)
```

The ledger records five failures. At 20:14 UTC, read-only `ls-remote` queries
succeeded against origin (1.19s) and GitLab (1.49s), returning different tips.
That does not prove a previously timed-out large push now succeeds, or that
credentials, object transfer, hooks and forge health are all good.
**Unapplied recommendation:** inspect the retained timeout incident and GitLab
service/SSH transfer health, confirm local/remote ancestry and outgoing payload,
then authorize an ordinary retry-budget reset. Do not raise timeouts or force
push merely because an advertisement succeeds.

### browser-extensions-shared: actual product gate refusal

Journal at 20:54:53 BST captured all seven affected workspaces running their
four gates, followed by:

```
Pre-push: 4-gate FAILED for:
  extensions/standalones/SamAI/LINT exited 1
```

The referenced private attempt directory had already been removed when inspected;
its individual lint diagnostic is unavailable. The journal still establishes
the failing gate, not its exact lint rule. At 20:14 UTC origin advertised its
branch, while the local `gitlab` alias was absent (exit 128). Existing daemon
remote reconciliation/backoff can remove aliases; this inspection did not add
one or infer that the forge itself was unavailable.
A subsequent read-only `timeout 180 bun run --cwd
/home/dracon/Dev/browser-extensions-shared/extensions/standalones/SamAI lint`
returned zero during post-deployment investigation. The project requires Bun;
an initial pnpm invocation refused before running lint. Thus SamAI's historical
lint refusal is proven, but is **not** claimed still reproducible or fixed by
this change. The full queued multi-workspace four-gate has not been rerun
manually. **Unapplied recommendation:** retain all four gates, inspect current
mirror reconciliation and exhausted retry state, then authorize an ordinary
retry after the applicable product gates pass. This utility change does not
claim to repair SamAI.

## Rendering and regression requirements

The banner labels concerns separately from inspection failures and calls its
row count “inspected”. A failed-push age with two digits previously exceeded
the rich PUSH column's ten-cell content area. The PUSH column now sizes to the actual rendered labels, funding extra cells
from the flexible repository column. Both FAIL and the full age stay on one
line without changing the shared vocabulary or increasing the rich-tier floor.
An initial icon-spacing change failed an existing history/convergence test;
the implementation was replaced, not that test weakened.

Regression coverage protects successful filters lasting longer than the old
UI deadline, existing timeout/owner-abort cleanup, exact detail selection,
non-promissory UNKNOWN hints, failed-push age width, and lease contention without
commits or loss of failure memory (including a seeded paused remote and nonzero
failure count). Checkout-identity regression coverage rejects copied metadata
while preserving alias edits and accepting a symlink to the same real worktree. Existing filters, hooks and push accounting
remain enforced. The successful slow-filter fixture and existing hung-filter
fixture distinguish harmless slowness from real stuck work.

## Evidence and verification

Baseline and diagnostic captures are retained in `evidence/repos-reliability-2026-10-22/`.
Reproduce read-only diagnosis with:

```
timeout 180 dracon-sync repos
# detail inspection is restricted to this target in the corrected build:
timeout 90 dracon-sync repos ai-auto-writer
systemctl show dracon-sync-operator.service -p ActiveState -p MainPID -p TasksMax
journalctl -u dracon-sync-operator.service --since '2026-10-22 14:26:55' --until '2026-10-22 14:27:01'
git -C /home/dracon/Dev/dracon-platform/web/games/wip/aud-vis rev-parse --git-common-dir --show-toplevel
```

The focused CLI fixture `scripts/test-repos-detail.py <binary>` injects a real
status refusal in a disposable watch tree and requires the actual error, not
“not found”; its success case tolerates an unrelated extra repository. The old
installed binary fails this negative control with the misleading absent-repo
message; the candidate debug binary passes both cases. No live repo is mutated
by this fixture.

A read-only reviewer found the lease-backoff test initially lacked seeded
remote failure state. That coverage gap was corrected: both remote maps and a
nonzero failure count are now asserted preserved. The implementation itself
already retained them. Follow-up review covers the checkout-identity guard.

## Initial qualified rollout

All qualified source files and workspace manifests matched the source-digest
manifest captured before final verification; no source drift was observed.

- `timeout 1500 env CARGO_BUILD_JOBS=1 cargo test --workspace --locked`: exit 0;
  2,550 passed, zero failed, 25 pre-existing ignores across 37 result groups.
- `timeout 1200 env CARGO_BUILD_JOBS=1 cargo build --release --locked`: exit 0.
- `timeout 600 env CARGO_BUILD_JOBS=1 cargo clippy --workspace --locked -- -D warnings`: exit 0.
- `timeout 300 cargo deny check`: exit 0; advisories, bans, licenses, sources OK.
- `timeout 30 cargo fmt --all -- --check`: exit 0.
- Release and installed CLI detail fixtures both passed. The installed
  `repos aud-vis` exits nonzero with the actual worktree mismatch, not absence.

The earlier workspace failure was the preserved label-vocabulary test, corrected
by replacing icon compaction with content-derived column sizing. A targeted
standalone inspection run later hit its compilation/file-lock deadline; that
is not claimed a functional pass. The final full workspace run includes all
new compatibility/alias/cancellation tests and passes. Full-build wall time was
substantial under host memory pressure; build concurrency remained one.

Deployment used the sanctioned maintenance wrapper, backed up both the
installed and separately loaded executable outside daemon-home, atomically
replaced only dracon-sync, and signalled the owned daemon through a PID file
descriptor. The system unit's existing `Restart=always` restarted it; there was
no service stop, unmask, root-unit edit, or privilege escalation. Maintenance
removed its freeze marker and resumed normal sync.

- Restart: 2026-10-22 20:56:34–45 UTC; PID 3228336 → 319430.
- Qualified artifact, installed CLI and `/proc/319430/exe` SHA-256:
  `ad73e71dbc5d6cab141e8e0421bf8f706124e3ee9b6955a9cafc4607246bb80a`.
- Rollback/receipt directory:
  `~/.local/state/dracon-sync/deployments/20261022T205632Z-repos-reliability/`.
- `evidence/repos-reliability-2026-10-22/deployment.log` records actual loaded
  identity, not merely installation intent. Repeated observation receipts below
  check that identity and the absence of a freeze marker again.

No manual sync/recovery commands, manual pushes, history rewrites, force pushes,
repository deletions, checkout repairs, hook bypasses, or coverage reductions
were used. Normal daemon activity and disposable test-fixture operations are
not manual recovery. Product lint was inspected without `--fix`.

## Observed autonomous behavior and remaining recovery

Three fleet snapshots cover **20:57:20–21:03:58 UTC** (6m38s), longer than the
300-second status retry interval. Each CLI call exited zero in 29–35 seconds;
each receipt reports active PID 319430, TasksMax=512, the qualified loaded
SHA-256, and no freeze marker. All snapshots show 36 successfully inspected
rows and exactly one explicit inspection failure: aud-vis's mismatched
worktree. No status-timeout event appears in the captured deployment journal.
This is bounded observation, not a promise that every future inspection or
remote operation will succeed.

| Target | Actual observation | Disposition |
|---|---|---|
| ai-auto-writer | Daemon committed 15 files at 20:57:38 UTC, synced at 21:00:46, and committed 20 subsequent edits at 21:01:56. Final row remains working with one newly ahead commit. | Inspection timeout resolved; real autonomous commit/push progress demonstrated, not perpetual cleanliness. |
| dracon-platform | Committed seven files at 20:58:00, synced at 21:02:03, then committed thirteen subsequent edits at 21:03:18. | Inspection timeout resolved; autonomous progress demonstrated despite ongoing edits. |
| aud-vis | Daemon rejected neighboring-worktree resolution at 20:56:56 and again at 21:02:03; retry remains 300s. The CLI names both paths and counts the failure. | Unsafe alias no longer appears healthy or schedules neighbor work. Checkout/content reconciliation remains unapplied and requires approval. |
| capture-anime-girls | One ahead commit at first snapshot; zero ahead and converged in the final two snapshots, with a fresh successful push observation. | Canonical worktree remains covered; rejecting the alias did not remove its sync coverage. |
| doomtap | All three snapshots show zero pending/ahead commits and a successful recorded push predating this restart. | Independently recovered during investigation; no doomtap policy or ledger repair was performed here. |
| dracon-strategy | Ordinary commit-only processing continues, but 51→52 ahead commits remain; final status STALLED and last push still failed. | GitLab timeout/exhausted retry state remains unresolved; no manual reset or push was authorized or used. |
| browser-extensions-shared | Normal commit-only activity continues; ahead backlog 31→36. No new successful push observation appeared; final status WORKING describes current worker ownership, not successful publication. | Backoff/exhaustion and full-gate recovery remain unverified/unapplied. Historical SamAI lint refusal was subsequently followed by a successful read-only lint run, not by a claimed full-gate/push recovery. |

The one intermediate UNKNOWN observation for newly edited ai-auto-writer is
retained in raw evidence, not relabelled healthy: its successful prior push
cannot establish current dispatch during a gap in affirmative scheduler
phase evidence. Its hint no longer promises unobserved convergence, and the
next sample observes a real working owner.

### Operator-only follow-up, not executed

1. Compare the aud-vis directory's content against capture-anime-girls before
   deciding whether it should become a distinct checkout or an intentional
   alias. Approval must cover the actual content-preserving checkout repair;
   do not delete either directory or silently remove it from coverage.
2. For dracon-strategy, inspect local-hook and GitLab transfer/ancestry/payload evidence, then
   authorize `dracon-sync repair stuck-unstuck dracon-strategy` only after the
   cause is addressed. The reset command itself does not fix a transport fault.
3. For browser-extensions-shared, verify the applicable complete four-gate
   and current mirror setup before authorizing
   `dracon-sync repair stuck-unstuck browser-extensions-shared`. Passing SamAI
   lint alone is not sufficient evidence to waive the other gates.

These are recommendations, not permission requests hidden in an implementation
claim. Remaining repository-state recovery was explicitly excluded from this
run. None of the unresolved targets is described as converged.

## Current-source requalification and second rollout

Concurrent work added a max-tracking-ref regression and a local-operation-timeout
classifier while the initial receipts were being assembled. The maximum-count
production implementation was already present; the added test checks that the
furthest-behind tracking ref determines the count. The timeout classifier
identifies the runner's deadline format, distinguishing it from forge-side
outages and ordinary transport failures, without altering retry accounting.

The classifier's initial wording asserted remote reachability and prescribed
larger timeouts from only the deadline message. This run corrected that wording
and its explanation: a local deadline event alone cannot establish whether a
hook or network transfer stalled. Guidance now asks for hook/transfer progress
and budget inspection and explicitly leaves remote health unestablished. It
neither increases budgets nor bypasses hooks. The independent reviewer accepted
the change and the four additional regression cases.

The **current** source, including those additions and the truthful timeout
wording, was requalified from a fresh source-digest manifest:

- Full workspace tests: **2,554 passed, zero failed, 25 existing ignores**, exit 0.
- Release build, Clippy with warnings denied, dependency checks, and formatting
  checks: exit 0.
- Current release and installed CLI detail fixtures: both cases pass.
- Raw current receipts: `workspace-current.log`, `build-current.log`,
  `clippy-current.log`, `deny-current.log`, `format-current.log`, and
  `detail-installed-current.log` in the evidence directory. Earlier receipts
  remain historical; they are not reused to qualify a changed artifact.

The second maintenance deployment at **22:38:37–48 UTC** replaced PID 319430
with **3170257** through the same atomic-install/owned-SIGTERM/Restart=always
procedure. The artifact, installed CLI and loaded daemon matched SHA-256
`f697cb28ec4b7824b6a31c60256925b895316c6bea0fe44b29829bd5f989a3a6`.
Rollback copies and receipt are at
`~/.local/state/dracon-sync/deployments/20261022T223836Z-repos-reliability/`.
The freeze marker was removed normally. No additional repository recovery was
performed for this requalification or rollout.

