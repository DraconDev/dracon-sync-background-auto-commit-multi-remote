# Qualification checkpoint — fleet convergence remains open

**Historical checkpoint:** this records the first qualified deployment. Later
work and qualification are tracked in [execution-update-2026-10-24.md](execution-update-2026-10-24.md).
In particular, Warden has since been deployed again, strategy has published,
platform publication has progressed, and deathrun's initial image staging
conflict has been repaired. Statements below marked pending describe this
checkpoint, not the current observed state. No final convergence claim exists.

## Current runtime

The operator service now runs PID 18115 with executable SHA-256
`f08e08a242057ceedb2bcf67e4670a1e18c130f2520a3c4f4aaa2928f2f8b6cb`.
`checks/daemon-qualified-install.log` verifies the loaded executable, not merely
the destination pathname. Deployment used the existing maintenance wrapper,
atomic replacement, executable backup, pidfd-targeted SIGTERM and the unit's
Restart=always. The maintenance wrapper removed the freeze marker afterward.
No service stop or manual repository push was used.

The qualified Warden executable is
`1e42b6f47b441459a3b1ec4733f29cb3f3f1cb14f3f027fab1ae3dcbe465527f`.
Before its replacement, all three live global hook scripts already matched the
qualified renderer byte-for-byte. This installation replaced the older binary,
with a backup, to prevent future stale hook rendering; it did not rewrite the
live hook files or change security settings. The earlier precision-repair
regressions remain applicable. Strategy publication is NOT established by this.

## Qualification results

- `cargo test --workspace --locked`: 2606 passed, zero failed, 25 ignored.
- `cargo clippy --workspace --locked --all-targets -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- `cargo deny check`: advisories, bans, licenses and sources passed.
- Platform bucket suites: 173 passed, zero failed across 11 Bun test files.
- Platform private-transfer suite: 20 passed, zero failed under Node.
- Read-only fleet observer regressions: four passed.

Raw results are in `checks/*qualified.log`, `checks/*final-patched.log`,
`checks/workspace-final-patched.log`, `checks/release-final.log` and
`checks/observer-tests.log`. These are bounded qualification results, NOT a
claim that every affected repository's real product gates have passed.

The dependency check initially found RUSTSEC-2026-0386 in Tokio 1.53.1. The
workspace lock now selects the minimal fixed patch, 1.53.3, without an advisory
exception; the workspace suite, Clippy and release build were rerun afterward.
Two semantics-preserving Clippy corrections in concurrently developed
`dracon-system/src/reap.rs` were needed for the workspace gate (`?` and reverse
sort key). No claim is made to have implemented that unrelated process-reaping
feature. Existing unrelated formatting differences were not bulk-reformatted.

## Storage preparation correction

`bucket-private-workflow.mjs` previously measured the complete reachable object
graph before every upload, even when every manifest entry was already enrolled.
Existing entries are routed regardless of pressure, so that measurement cannot
change their decision and needlessly consumes the finite preparation deadline.
The workflow now uses an explicit existing-references-only mode in that case.
No fabricated zero-pressure measurement, larger timeout, skipped byte check or
weaker provider ceiling was introduced. New candidates still require the real
measured budget; manifest changes that introduce a new candidate fail closed in
this narrow mode. Commit/push high-water guards remain unchanged.

The transfer still checks remote existence and downloads/verifies remote bytes
on each attempt, including unchanged references with a valid local cache. An
additional reproduced race showed unchanged references skipped the final local
source verification. Every enrolled reference now gets that verification; a
concurrent source edit refuses publication and retains the edit and old
manifest. A subsequent attempt can upload the new immutable object while
retaining the old object. Both defects have failing-before/passing-after
regressions retained in `checks/`.

Five unrelated-to-the-new-routing-branch bucket suite failures were fixture
integration drift: the parent-hook fixture did not copy the actual canonical
append-only guard, a nested policy retained a bucket with no matching namespace,
and synthetic music manifests lacked an explicit public namespace/bucket
mapping. Fixtures were corrected; the real guard and all original assertions
remain active. Production bucket configuration was not relaxed.

One broad Node invocation incorrectly included Bun-only tests. A subsequent Bun
invocation without explicit `./` paths scanned too broadly; another run observed
transient ENOSPC and a concurrently replaced Bun executable. The retained passing
run uses explicit test paths and a private executable copy. These failures were
resolved and are not classified as external blockers.

## Capture lifetime and executor stack

The new owned workflow-capture path exposed a debug-stack overflow in the real
blocked-publication actor regression. The capture reader embedded an 8264-byte
future containing its I/O array, multiplied through the surrounding async state
machines. The scratch buffer is now heap allocated. Its size regression fails
before the correction, and all three production-entry publication actor tests
pass afterward. Stack limits, deadlines and concurrency assertions were not
increased or weakened.

## Installed executable receipt

`checks/isolated-installed-receipt.json` was produced by executing the installed
binary, not a test executable. It reports the same SHA-256 as the loaded service.
Its private HOME, state, policy, watch root and filesystem-only bare remotes live
under `/tmp/fleet-qualified-isolated-installed/fixture`, outside real watch roots.

The receipt verifies:

- inherited PATH lacks Bun/pnpm, but B's real commit hook resolves and executes
  both from the fixture HOME;
- A remains blocked in its real push hook while B commits and publishes the
  specifically requested work, not merely its setup baseline;
- another real A commit lands during A's own blocked publication, and follow-up
  publication reaches its newest tip;
- all ordinary commit lifecycle hooks run in order;
- C's refusal preserves work and blocks both commit and publication; after its
  explicit fixture cause is repaired, autonomous retry publishes a real commit;
- repository-local ownership leases exist, SIGTERM shutdown joins cleanly, and
  no fixture processes remain by command-line OR working-directory inspection.

Fixture hook scripts, control markers and trace files now live in Git metadata,
so recording a receipt does not itself manufacture further eligible commits.
A staging-artifact receipt also passed before deployment. Neither receipt proves
fleet-wide convergence or the final state of a product hook on a live repository.

## Live evidence and remaining work

`checks/refs-before-deployment.json`, `checks/refs-first-loaded.json` and
`checks/refs-final-loaded-v2.json` contain advertised refs and local commit-object
checks for ALL ten initially affected repositories. Each pass observed one
stable loaded executable revision and zero failed ref queries. The first two
snapshots' initial target annotation copied the mirror-only CLI list; retain
primary origin as required too. The v2 observer explicitly includes origin.
Logical mirror names still need destination/alias reconciliation when producing
the final ledger: `.dracon` reaches GitHub through origin, without a separate
configured `github` alias. No remote should be added or rewired to fix that.

These snapshots are not the final convergence ledger. Several repositories had
already reached their remotes before this deployment; unchanged advertised refs
do not establish new autonomous commits on this loaded revision. Active editing
and sequential per-remote observations can also produce short-lived differences.
Require object/journal and before/after evidence, not only status labels.

Still demonstrably unresolved:

1. Browser extensions and strategy retain exhausted publication retries. The
   installed environment/revision-isolated hook fixes do not themselves prove
   their current coherent source gates pass. Verify the real gates, then use only
   targeted `repair stuck-unstuck <repo>` after repairing each cause.
2. Platform still has an unpublished backlog. Observe actual preparation and
   hook results on the new daemon; activity is not convergence.
3. Deathrun's high-water and forward-only checks pass, but Warden's **non-security
   machine-local staging check** rejects 12 PNGs under an audit-evidence run
   (before, after and generated source intermediates). All 12 remain on disk and
   all 12 currently fail `git check-ignore --no-index`: removing them from the
   index alone would let the daemon re-add them and is NOT a durable repair.
   The check is in the global Warden pre-commit template, not a deathrun-local
   hook. Inspect the run's actual generator/report dependencies before choosing
   a documented, behaviorally verified non-security hygiene correction. Do not
   exempt the hook, weaken secret checks, silently exclude deliverables, delete
   local images, or apply its one-time index-removal hint without fixing the
   underlying policy mismatch.
4. Runtime push-stuck journal/alert text still offers generic
   `repair concerns --apply`, even though the status-row hint is now targeted.
   Bring those runtime hints into agreement without invoking generic repair.
5. Complete coherent affected-product qualification and final-revision autonomous
   commit/publication evidence for every initially affected repository. No
   external blocker has been accepted, and no completion claim is submitted.
