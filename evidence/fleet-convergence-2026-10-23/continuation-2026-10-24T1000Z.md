# Active convergence checkpoint (2026-10-24 10:00 UTC)

Goal remains active; no completion submitted. The actual contract is
`.pi-glla/goals/20261023154754-ofnb5c.md`; `.pi-glla/handoff-brief.md` is stale
and describes an older bucket task.

## Runtime and live proof

Service PID3393180 still runs installed `/home/dracon/.local/bin/dracon-sync`,
SHA256 `8be3718f27c1bc6edee5bb38a27d3911fdb1154715848e148dc13009cb5891cb`.
Warden installed artifact remains c0f1594c5890cbfc08f39ba2bda20072cafcd0ec183d757c2c39af6563470787.
`checks/live-convergence-ledger.json` now proves9of10 on that artifact; only
browser-extensions-shared remains nonhealthy-unproven. Fresh read-only refs
were captured in `checks/refs-final-loaded-after-books-retirement.json`, copied
to the current after-work snapshot. Journal was refreshed for the loaded PID.

Polis renamed its eligible note to AUDIT-SYNC-CONVERGENCE-2026-10-24.md after
creation; do not undo that edit. New durable scripts/fleet-convergence-ledger.py
and fleet_convergence_witness.py select the exact historical blob with the same
ancestor/time/PID-event checks. Six pure regressions pass. Original temporary
builder is `/tmp/fleet-build-current-ledger.py`; it has the latest-path bug and
must no longer generate the canonical ledger.

## Repairs since the previous summary

- Books parent was blocked by4446 pending old top-level sample deletions. Full
  evidence proves4414 retained identical canonical copies and32 samples from8
  slugs absent from current shelf/catalog in all3views. Existing sanctioned
  retiredRoots mechanism now records ONLY static/books/*/chapters/*.md. Canonical
  shelf, other formats/deeper paths and every other guard remain protected.
  Full forward-only suite19tests/75assertions passes before/private-candidate
  and after adoption. Independent reviewer c7ecaeb1 found no issue. Docs/evidence
  are in platform docs/design/books-legacy-sample-retirement-2026-10-24.md and
  web/docs/books-legacy-sample-retirement-2026-10-24.json. Normal daemon publication
  now proves the parent's eligible note on all intended destinations.
- Browser three-product gates all passed on coherent0aa83f83 earlier. Targeted
  retry at05:05 exposed actual service postinstall native Tokio EAGAIN/SIGABRT
  under TasksMax512/CPUQuota200%, not a forge problem. Five retries exhausted.
- Shared browser run-push-gates.mjs now caps native child pools (Tokio/Rayon/
  Rolldown/GOMAXPROCS2, Rolldown blocking8), respecting stricter values and
  retaining them in receipts. No lifecycle/gate/deadline/concurrency bypass.
  All19 git-hook tests pass, including two new all-phase environment regressions.
  Fixture hook/signing suppression was removed. Real binding observation13vs23
  threads is NOT claimed as a full failure reproduction.
- Service-shaped private qualification (512tasks/twoCPU verified via cgroup)
  passed both installs,7library builds and all12selected product gates on
  a5b450c29f836eac045a51f14a828e4edf3e16c8. Raw logs/recipe at
  checks/native-pool-qualification/qualified-products/. This is not the whole
  actual pre-push/mirror-publication proof yet. Do NOT re-enable browser until
  final binary deployment, to avoid cancelling a long hook during restart.
- That qualification initially caught a subsequent unrelated extension-core
  catalogue addition missing its existing OPENROUTER_API import. One-line
  import repair plus mocked-fetch regression fail-before/pass-after; core tsc
  passes. No API request or new feature was implemented.
- New push.rs diagnostic wording removes the false instruction to fix storage
  policy for every local hook failure; it now includes environment/product
  inspection and warns against weakening policy. Classification/budgets unchanged.
  This changes the daemon artifact, so final deployment/installed receipt and
  all10 final-revision witnesses MUST be refreshed after qualification.
- Independent reviewer9b7219d6 found no issue with these final deltas. Exact
  scoped diffs/sources in checks/review-final-slice/. Managed report is in that
  review's async directory; archive it before retention. No fixes requested.

## Current qualification obstacle (ordinary work, NOT external)

`checks/workspace-final-v4/receipt.json` is FAILED and must NOT authorize deploy.
It ran on mutable live sources while a separate dracon-system agent compiled/
edited. Two unchanged pre-existing storage-library tests failed with nonblocking
journal-lease busy/unavailable; the same source files are identical to the
previous all-passing v3 ref9869cd0039. Clippy caught a new dracon-system
main.rs sort_by idiom; repaired semantically to sort_by_key(Reverse). Release
and deny passed, but input fingerprint changed, so the artifact is unqualified.
Other active cargo jobs belong to unrelated work; never kill them or delete
canonical target/checkouts. Storage lease guards/assertions were NOT changed.

Coherent qualification v5 was rejected after real failures; corrected v6 is now
RUNNING: checks/workspace-final-v6/{source,receipt}.json.
See v6/source.json for its immutable private source location. The earlier
/tmp/fleet-workspace-frozen-2ajly5n9/source is retained as a private build cache.
Both are ordinary Git checkouts at captured refs with existing required smudge
filters copied exactly.
Private bare origins and object alternates read canonical objects without
changing canonical refs/config/checkouts. The build has its own target directory.
RUST_TEST_THREADS=1 explicitly serializes test cases, not internal publication
concurrency; no assertions, ignored cases or production locks were changed.
A controlled fork/CLOEXEC lease reproduction demonstrates a parent-close can
remain busy until a pre-exec child exits, consistent with the earlier fixture
failures but not proof of their exact PID. Raw reproduction and recipes are in
v5/. Do not claim a default-parallel pass or deploy until all4gates pass.
Previous v3 source refs remain in checks/qualification-v3/source.json.
V5 serial scheduling passed the formerly busy lease fixtures without altering
storage code/assertions, but caught (1) an actual classifier bug: the existing
forge predicate treated local worker EAGAIN's 'temporarily unavailable' as forge
outage, outranking local-hook reporting; and (2) missing normal Node SDK setup in
the new checkout. Push.rs now narrowly excludes explicit local-hook worker-spawn
failures from forge classification; real remote Gitaly equivalents stay transient,
with a counterexample regression. V6 freezes that committed fix, runs normal
locked npm ci for scripts/bucket-runner (no lifecycle bypass), and reuses ONLY
the owned private v5 compilation cache. V5's active private compiler was cancelled
by its owner via pidfd; unrelated jobs were not signalled. V6 raw workspace.log
now shows the corrected resource-classification test and all3production bucket
actor tests passing. Full gates remain pending. A targeted read-only follow-up
review is running as76a8342c-4520-49d9-b492-af19bbf4d027, checking only the narrow
forge/local-worker classifier delta; the dependency is owned by this goal.
Prepared v6 stage-and-receipt.py, deploy-qualified.py and write-eligible-refresh.py
are NOT executed yet. They require passing gates/candidate execution; all fixtures
and candidate ELF stay outside watch roots. Deployment template was recovered
from the prior successful transaction. V6 staging also checks dynamic libraries
are not tied to disposable build paths. The new pre-work snapshot/eligible files
are named refs-final-v6-before-work.json and final-v6-eligible-work-witnesses.json;
archive old8be epoch and promote only after the new deployment/installed receipt.

After all4Rust gates pass on a coherent assembly: run candidate isolated receipt
using scripts/isolated-publication-receipt.py, safely deploy via maintenance +
atomic replacement + pidfd SIGTERM (Restart=always; no service stop), verify
/proc executable hash, run installed receipt, capture a fresh before-ref baseline,
write useful eligible updates for all10 respecting renamed note paths, re-enable
ONLY repaired browser retry, and obtain original whole-gate and advertised-ref
proof on that final loaded artifact. No manual recovery pushes, history rewrites,
force pushes, checkout deletion/rewiring or security-policy changes are authorized.

Tasks3.2 complete;4.2 reopened for the upcoming artifact. Others remain in progress.
