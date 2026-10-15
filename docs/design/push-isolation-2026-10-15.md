# Push-hook isolation — implementation checkpoint (not completion)

## Goal

Isolate heavy pre-push work from inspection/commit scheduling with scoped
resource/concurrency controls. Preserve real hook enforcement and interactive
workloads; no new memory caps or blanket service-limit increases.

## Current implementation

- `src/git/push_resources.rs` introduces daemon-worker-local work ownership.
  `with_local_work` owns the existing semaphore permit. The whole
  `sync::push_background` phase returns that permit while network/hook work
  runs and reacquires it before subsequent local work.
- The phase boundary encloses concurrent mirror futures; per-command permit
  restoration would be incorrect because one mirror could restore local
  capacity while another mirror's hook was still running.
- Actual `push` commands in the scoped phase have a separate one-command
  semaphore. `ops.rs` gates all three common asynchronous timeout runners
  before spawning, rather than spawning children then waiting on capacity.
- Only scoped owned push children receive nice 19 through `pre_exec`; Git's
  hook descendants inherit it. Parent daemon/CLI priorities and service limits
  are unchanged. Hooks are neither disabled nor run independently of Git.
- Cancellation ordering is explicit: an OwnedCommand struct drops its boxed
  process-running future before its push permit, so the existing owned process
  group guard signals the hook tree before another push can enter the lane.
- CLI/manual operations outside `with_local_work` are intentionally unaffected.
  Existing per-repo reservations remain held until actual worker join.

`timeout 180 cargo check -p dracon-sync --locked` currently exits 0. This is a
compile checkpoint, NOT proof of the verification contract. No deployment.

## Next implementation / verification work

1. Add real Git bare-remote fixtures under `push_resources` (or the sync test
   module for direct `push_background` access). A slow executable pre-push
   publishes its own/helper PIDs then blocks on explicit release. Use the real
   scoped production entry, not fake Git output or sleeps as ordering proof.
2. With local capacity one and the hook known entered, inspect an unrelated
   dirty repo and commit exact expected content through normal local admission.
   Both complete before the release marker is written; hook remains pending.
3. Accepting release must update bare refs; rejecting release must fail and
   leave refs unchanged. Existing hook-enforcement fixture covers normal,
   named-remote and maintenance push routes and must remain passing.
4. Abort an owned slow push with a TERM-resistant helper. Strictly verify
   recorded hook/helper identities are stopped and a separately grouped
   interactive sentinel still answers a pipe handshake. Reuse lane afterward.
   Give every harness-owned task/child a cleanup guard, including panic paths.
5. Check child nice value and unchanged parent/sentinel priority. Verify local
   permit restoration after ordinary failure, capacity during concurrent mirrors,
   and bounded command concurrency.
6. Reconcile the scout's additional push routes: synchronous branch deletion,
   ordinary timeout/env runners, concern dry-run capture, and coordinator
   maintenance vs daemon-worker scope. Do not claim those are isolated merely
   because the main push path is enrolled. Inspect which can be live daemon
   heavy-hook paths before deciding the safe necessary integration.
7. Run bounded focused/workspace tests, Clippy, release build and advisory
   checks, then a fresh read-only reviewer. Keep known rustls findings separate.

## Research evidence

Read-only scout report:
`/home/dracon/.pi/agent/sessions/--home-dracon-Dev-dracon-utilities--/subagent-artifacts/outputs/5a588b19-b043-40f1-8730-174b277f79a9/context.md`.
Reusable real-hook enforcement fixture: `src/git/push.rs:773–864`.
Owned cancellation/strict process-state fixtures: `src/git/diff.rs:767–876` and
`src/git/ops.rs` tests. Scout finished; no research agent remains running.
Goal tasks: trace complete; implementation/tests in progress; final verification
and review pending. Resource design judgment recorded inline in the goal ledger.
