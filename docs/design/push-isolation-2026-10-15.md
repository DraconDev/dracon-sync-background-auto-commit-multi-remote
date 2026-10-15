# Scoped pre-push isolation — 2026-10-15

## Contract

Isolate heavy pre-push work from inspection/commit scheduling with scoped
resource/concurrency controls. Preserve hook enforcement and interactive work;
no new memory caps or blanket service-limit increases. A bounded real-hook
fixture must demonstrate unrelated inspection/commit progress and owned
hook-descendant cleanup on cancellation.

## Implementation

`src/git/push_resources.rs` supplies two separate resource lanes:

1. **Local work:** the existing daemon semaphore and its configured capacity.
   `daemon.rs` enrolls each worker's acquired permit with `with_local_work`.
   A push phase returns that permit while hooks/network work run and reacquires
   it before any subsequent local mutation. The per-repository scheduler
   reservation is NOT released: another worker cannot mutate that same repo.
2. **Push commands:** one daemon push command at a time, separately from local
   work. `ops.rs` gates all three common async timeout runners before spawn.
   Only push children in the scoped phase get nice 19 through `pre_exec`;
   hook descendants inherit it. The daemon's and interactive workloads'
   priorities are untouched. No memory, CPUQuota, task-limit or service changes.

The outer normal push phase encloses all concurrent mirror futures. Releasing
and restoring the shared local permit independently for each mirror would be
wrong: one mirror could restore the permit while another still runs a hook.
Nested phases reuse the enclosing scope. Individual command admission retains
its own one-command bound.

`OwnedCommand` owns its boxed runner before its command permit in declared
field order. Cancellation drops the runner and its process-group kill guard
before releasing push capacity. Existing timeout and owned-descendant cleanup
remain in force. CLI/manual commands outside daemon enrollment remain unchanged.

### Enrolled paths

- `sync::push_background`: normal origin and mirror pushes, including their
  retry/transport fallback work.
- `release::create_and_push_tag`: tag creation remains local; the actual push
  releases local capacity, restoring it before failure rollback.
- `git::maybe_prune_stale_backup_branches`: the opted-in, bundle-first janitor's
  actual remote deletions use the same scoped phase.
- `nix::run_git_for_nix_pr`: branch preparation/commit remain local; the actual
  push yields capacity and restores it before checking out the previous branch.
  Its existing release-pipeline caller awaits it without detaching task context.
- Default-branch consolidation/rename can also push before normal inspection.
  They now run as one owned per-repo background maintenance job, excluded from
  sync dispatch until joined, with a 300-second retry cooldown if still needed.
  Their push scope uses the same one-command lane and child priority. Remote
  branch deletion was converted from synchronous output/spawn_blocking to the
  existing bounded, process-group-owned async runner.

The pre-existing narrow maintenance rewrite exception is preserved ONLY on its
existing deletion paths. No new rewrite override, hooksPath override,
`--no-verify`, content-check skip or global identity/config change is introduced
in production. Existing size/ownership/bucket guards are unchanged. Diagnostic
CLI and manual push commands are intentionally not enrolled as daemon workers.

## Concrete verification

New fixtures are Linux-gated, matching the deployment platform. Readiness is an
explicit published PID marker; release is an explicit marker controlled by the
parent. Polling sleeps only observe predicates, never assert ordering.

| Contract | Production-path evidence |
| --- | --- |
| Slow normal hook cannot starve other repos | `slow_push_hook_leaves_capacity_for_inspection_and_commit_and_enforces_rejection` invokes real `push_background` with local capacity one. While the hook is entered and no release exists, another repo acquires local capacity, passes real `inspect_status`, stages explicit content and commits it; HEAD and committed bytes are checked |
| Release/janitor/Nix paths also yield | `release_tag_janitor_and_nix_hooks_yield_local_work_capacity` exercises the actual tag function, opted-in bundle-first janitor and actual Nix Git preparation/push function against a local bare remote. Each blocked hook coexists with a healthy inspect+commit. Final tag/deletion/branch, bundle backup, Nix branch restoration and returned permits are checked |
| Hook enforcement preserved | Accepting hook publishes the bare ref; rejecting hook leaves it unchanged. Existing `daemon_push_paths_honor_pre_push_hooks` also passes for normal, named-remote and maintenance paths |
| Owned cancellation cleanup | `cancelled_push_hook_cleans_owned_descendants_but_not_interactive_sentinel` aborts the owner, awaits typed cancellation, verifies hook and TERM-resistant helper are stopped, then successfully reuses the push lane |
| Interactive workloads preserved | A separately grouped shell sentinel answers pipe handshakes before and after cancellation, retaining its priority. Parent priority is unchanged; hook/helper priorities are actually 19 |
| Bounded resource admission | `hook_command_lane_is_single_flight_and_drops_work_before_releasing_slot` explicitly polls a second push pending while the first owns capacity; cleanup must occur before the second enters. Inspection and unenrolled/manual commands remain runnable |
| Local mutation capacity restored | `push_phase_restores_local_capacity_on_error_and_nested_mirror_work` verifies restoration on error, no early mirror reacquisition, nesting and both lanes' returned permits |

Harness-owned tasks have abort-on-drop guards, including join timeouts. The
interactive sentinel has kill-on-drop and is explicitly killed/reaped on the
success path. Fixture-only Git setup uses isolated hooks/identities, never the
operator's repositories. Nix testing stops at the actual Git preparation/push
function and does not call an external GitHub API.

## Gates and evidence

Raw evidence: `evidence/push-isolation-2026-10-15/`.

- `timeout 240 bash -c 'cargo test -p dracon-sync --locked push_resources &&
  cargo test -p dracon-sync --locked push_isolation_tests && cargo test -p
  dracon-sync --locked daemon_push_paths_honor_pre_push_hooks && cargo test -p
  dracon-sync --locked git::branch'`: exit 0 (`focused-final.log`, before the
  late-entry regression was added).
- `timeout 180 cargo test -p dracon-sync --locked
  release_tag_janitor_and_nix_hooks -- --nocapture`: exit 0, all three added
  production entry points exercised (`late-entry-final.log`).
- Final `timeout 360 cargo test --workspace --locked`: exit 0; 35 successful
  suite summaries, 2,404 passed, zero failed, 25 ignored (`workspace-final.log`).
  This run includes the final Nix enrollment and all five new isolation tests.
- Final `timeout 180 cargo clippy --workspace --locked -- -D warnings`: exit 0
  (`clippy-final.log`).
- Final `timeout 590 cargo build --release --locked`: exit 0
  (`release-final.log`). Built, not installed.
- `timeout 120 cargo deny check`: exit 1 on the existing four rustls advisories;
  bans/licenses/sources pass (`deny-final.log`). Dependencies and exceptions
  were not modified by this item; remediation is separately queued.

## Independent review and repair

The first fresh-context read-only review confirmed the core design but found
incomplete enrollment of automatic tag pushes and janitor deletions (P1).
`review-initial.md` preserves that blocking review. Both actual operations were
wrapped, and the parent additionally enrolled the automatic Nix PR push path.
The real-hook regression now exercises all three cases, rather than relying
only on the main push fixture.

The resumed reviewer inspected the narrow fix and current source, marked the
P1 resolved, and found no remaining blocker (`review-followup.md`, OK with notes
pending final gates). The final workspace/Clippy/release runs above occurred
AFTER that follow-up and cover the final source. Review was independent source
and artifact inspection; all command execution was performed by the parent and
is attributed accordingly. `review-final.diff` captures baseline `38b6d6f` to
current source; `scout.md` preserves initial path/fixture research.

## Operating boundaries

This is bounded concurrency and child CPU priority, not a memory cap or a
promise that a single hook cannot itself allocate excessive memory. Required
hooks remain required and can still fail or time out. Owned ordinary process-group
descendants are covered; deliberately escaping a process group/session
is not a new containment guarantee. Existing scheduler ownership remains
fail-closed until worker teardown. No locks were removed and no daemon/service
was restarted. Deployment and live-soak observation remain separately queued.
