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
retry budget. No bucket policy or retry ledger was changed here.

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
**Unapplied recommendation:** in browser-extensions-shared run the SamAI lint
script defined in its package manifest, fix the actual product regression,
retain all four gates, and inspect current mirror reconciliation before any
manual recovery. This utility change does not claim to repair SamAI.

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

Quality-gate and deployment receipts will be recorded below after actual
execution. No passing full-suite or deployed-runtime claim is made by this
checkpoint.
