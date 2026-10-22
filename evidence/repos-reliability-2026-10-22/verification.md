# Contract-to-artifact checklist

Target: qualified current release/installed/loaded SHA-256
`f697cb28ec4b7824b6a31c60256925b895316c6bea0fe44b29829bd5f989a3a6`.
Current source digests: `current-source.json`; validation: `qualification-current.json`.
Paths below are relative to this evidence directory unless stated otherwise.

| Contract item | Evidence and observed result |
|---|---|
| Durable report covers every named symptom with raw evidence and bounded reproduction | `../../docs/design/repos-reliability-2026-10-22.md` (from this directory use `../../docs`); baseline `repos-before.txt`, Trace2 `status-trace.txt`, `doomtap-refusal.txt`, `strategy-timeout.txt`, `browser-refusal.txt`, `remote-inspection.json`, alias diagnostics and both rollout sections distinguish defects from unapplied state recovery. |
| Regression coverage and full workspace tests pass | `workspace-current.log` + `.exit`: exit 0; 2554 passed, zero failed, 25 existing ignores, 37 result groups. Covers slow filter success, timeout/owner cancellation, worktree alias rejection/content retention, real linked/non-UTF8/newline roots, failure semantics, max tracking ref, render widths, known detail failure propagation. `detail-installed-current.log` passes both real-CLI cases; `detail-old-negative-control.log` fails on the old CLI's actual regression. |
| Release build succeeds | `build-current.log` + `.exit`: exit 0. |
| Clippy with denied warnings passes | `clippy-current.log` + `.exit`: exit 0. |
| Dependency check passes | `deny-current.log`: advisories/bans/licenses/sources all OK; command ran under `set -e` before successful deployment. |
| Current loaded executable and bounded accurate CLI behavior | `deployment-current.log` records atomic replacement and loaded PID 3170257; three `observation-current/*-runtime.json` receipts independently repeat the qualified loaded digest, active service, TasksMax 512, freeze absent. Installed CLI fixture passes; all fleet calls return zero in 17–30s, each with 36 inspected rows and exactly one named mismatch failure, not a timeout or fabricated clean alias. |
| Actual autonomous progress; unresolved targets not called converged | `observation-current/journal.txt` plus Git object receipts verify current-binary commits in writer/platform/browser/strategy. `commit-object-verification.json` confirms 100/7/84/6 changed paths and ancestry in current histories. No current-window synced event is claimed. Raw three-sample row data retains browser's 37-ahead backlog, ongoing other work, and alias failure. Earlier successful syncs are explicitly bound to the first rollout, not reused as current execution proof. |
| Authorized actions and no destructive/recovery shortcuts | Report authorization/deployment sections; archived `deployment-procedure.py`, current receipt, backup locations outside daemon-home. No manual sync-now, applied repair, push, history rewrite, deletion, hook bypass, identity change, or coverage reduction. Operator configuration changes observed later are not attributed to this run. Product lint inspection omitted --fix. Disposable test fixtures are distinct from live recovery. |

Supplemental checks: `format-current.log` is empty with exit 0 observed under the
same `set -e` gate command; `git diff --check` passed. Read-only reviewer reports
retain the coverage notes and their subsequent resolutions; the last supplement
review found no source issues. The executor, not the reviewer, ran the full
quality gates and current deployment/observation.

Historical receipts remain labeled `*-final` and the first `observation/` tree.
They establish the earlier rollout only. One earlier workspace failure exposed
icon-spacing drift; implementation was replaced with content-derived sizing and
the existing history/convergence test was left unchanged. A standalone targeted
compile timed out waiting/building; it is not claimed as a passing test run.
All current source and behavior claims above use the later passing full suite.
