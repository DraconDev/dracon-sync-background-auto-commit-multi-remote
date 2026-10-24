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

Next: qualify an isolated coherent source assembly, with its own target directory
(no shared build lock or mutable-source drift). Previous v3 source refs are in
checks/qualification-v3/source.json. A bounded single-test-thread harness is a
possible qualification setting for these fork/OS-lease fixtures; preserve all
assertions and the actual internally concurrent publication tests. Do not waive
failures or claim a default-parallel pass. Diagnose/reproduce further if needed.

After all4Rust gates pass on a coherent assembly: run candidate isolated receipt
using scripts/isolated-publication-receipt.py, safely deploy via maintenance +
atomic replacement + pidfd SIGTERM (Restart=always; no service stop), verify
/proc executable hash, run installed receipt, capture a fresh before-ref baseline,
write useful eligible updates for all10 respecting renamed note paths, re-enable
ONLY repaired browser retry, and obtain original whole-gate and advertised-ref
proof on that final loaded artifact. No manual recovery pushes, history rewrites,
force pushes, checkout deletion/rewiring or security-policy changes are authorized.

Tasks3.2 complete;4.2 reopened for the upcoming artifact. Others remain in progress.
