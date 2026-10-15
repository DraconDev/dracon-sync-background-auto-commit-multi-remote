# Code Context

## Files Retrieved
Paths below are relative to `/home/dracon/Dev/dracon-utilities/dracon-sync/`.
1. `src/git/push.rs` (690–866) — real bare-remote hook enforcement fixture and maintenance route.
2. `src/git/ops.rs` (125–275, 310–464, 520–615, 730–844) — owned groups, common runners, cancellation and stdin synchronization fixtures.
3. `src/git/inspection.rs` (1–235) — actual inspection entry point and hung-filter recovery fixture.
4. `src/git/diff.rs` (755–892) — parseable-PID readiness, TERM-resistant helper, strict process-state verification.
5. `src/git/branch.rs` (1–69), `src/git/mod.rs` (5155–5206) — branch-deletion push paths outside progress runner.
6. `src/nix.rs` (211–241), `src/release.rs` (57–89), `src/report.rs` (7918–7951, 8416–8450) — additional push runner bypasses.
7. Scoped search of `src/git/multi_remote.rs` identified progress-runner calls at 658–660, 745–747, 778–781.

## Key Code
- `daemon_push_paths_honor_pre_push_hooks`, `src/git/push.rs:773–864`: real Git, temporary worktree + local bare remote + executable repository-local `pre-push`. Setup helper alone supplies `-c core.hooksPath=/dev/null` and explicit fixture identity; production calls honor local hooks.
- Same fixture covers `push_with_retries(..., retries=0)`, `push_to_named_remote(..., retries=0)`, and `force_push_after_rewrite` (`src/git/push.rs:826–836`). Rejection asserts error, hook marker, and **empty bare refs** (837–842). Acceptance asserts marker and bare main == local HEAD (843–863).
- Maintenance sets `DRACON_ALLOW_REWRITE=1` while retaining content checks/hook chaining (`src/git/push.rs:751–763`); normal/mirror expect 0 in the fixture.
- `run_git_with_timeout_env_progress` currently spawns through `spawn_git_command_env`, then calls the progress-aware owned runner (`src/git/ops.rs:531–542`). The env spawner creates a separate process group and enables `kill_on_drop` (426–445).
- `GroupKillGuard` sends SIGKILL to the **negative owned PGID**, not arbitrary descendants or all Git processes (`src/git/ops.rs:136–153`). `run_child_inner` keeps stderr capture owned and delays reaping leader until capture completes (155–174, 261–275).
- Captured inspection/classification uses the same group-ownership principle: `spawn_git_command_cancellable` plus `run_git_captured_output` (`src/git/ops.rs:343–408`). Inspection budget encloses the entire owned operation (`src/git/inspection.rs:11–43`).

## Architecture
Normal, named-remote, HTTPS fallback and maintenance pushes reach the progress runner; their real Git child executes pre-push and inherits its owned process group. Inspection has its own captured-output runner, not the progress runner. Thus push-resource isolation belongs around push execution/admission, without bypassing hooks or globally restricting inspections, commits, or interactive Git.
Parent is investigating production lane/semaphore wiring; this scout did not validate that wiring. A helper-only concurrency test would not prove daemon admission independence.

## Minimal Test Design
### 1. Real slow hook + independent repo inspect/commit
- Extend the real-Git fixture above; create repo A + bare remote and independent repo B, using explicit fixture identity and local hooks directories. Use actual production push entry points, not fake Git output.
- A's hook publishes a complete `entered` marker (temporary write + rename), then blocks on an explicit release protocol (FIFO/read pipe or marker predicate). It must not emit fake pack-progress lines.
- Exercise the **production lane admission path** with push capacity 1, and the minimum normal capacity that permits B. A owns the push lane until explicitly released.
- Await A's entered marker while concurrently polling A's push future (or use an owned task). Fail immediately if A returns before release. A timeout bounds fixture failure only; it is not an ordering assertion.
- Once entered, inspect B through `inspect_status`; assert its dirty counts. Run B's actual daemon inspect/commit path through its production reservation, using harmless content and explicit staged paths. Assert B's HEAD changed and committed bytes match.
- Before releasing A, assert no release marker exists, A has not completed, and B's inspect+commit completion signal arrived. This establishes overlap causally; elapsed sleeps do not.
- Release A; verify accepting hook completes and bare main == A HEAD. Also assert lane capacity is reusable after completion/cancellation through another admitted operation.
- For rejection, same slow protocol ends with `echo 'pre-push: fixture reject' >&2; exit 1`. Assert push error and unchanged bare branch tip (or empty refs for initially empty remote). Reuse all three existing routes to protect enforcement.

### 2. Owned cancellation + unrelated sentinel
- Adapt `src/git/diff.rs:767–864`: hook starts a TERM-resistant helper, records positive helper PID, and remains blocked. Publish readiness only when PID parses; file existence alone races redirection (804–832).
- Prefer recording PID + `/proc` starttime/PGID for identity-safe checks. Start the unrelated sentinel from the test harness in a distinct process group, **not from the hook**; otherwise it is legitimately owned.
- Await parseable PID/identity and sentinel-ready signals; abort the owning push task and await cancellation. Check hook/helper cease running using strict `/proc` handling from `src/git/diff.rs:866–876`: NotFound or Z/X means stopped; other read errors fail.
- Assert sentinel still responds to a channel/pipe handshake after owned descendants are gone. Give sentinel and fixture PIDs explicit cleanup guards; join/reap every harness-owned direct child even on assertion failure.
- Preserve leader-exited/stderr-holder regression: `src/git/ops.rs:772–840` deliberately lets leader exit while helper holds stderr, then aborts owner. Its 50 ms scheduling sleep (823–824) is not a reusable ordering proof; substitute an explicit runner-state signal/test seam if exact post-leader ordering matters.
- Strong event-gating example: `src/git/ops.rs:745–768` registers runner waits while child blocks on stdin, then explicitly releases stdin. Reuse this shape rather than a timed slow hook.

### Existing fixture limitations
- `src/git/inspection.rs:154–230` covers both owner abort and budget expiry, helper cleanup, then successful next inspection; reuse recovery assertion. It has a 1100 ms index timestamp sleep and permissive `/proc` errors, so prefer diff's changed-size input/strict checks for new tests.
- Existing cancellation fixtures inspected here have no unrelated-sentinel assertion. Existing real hook fixture proves enforcement, not slow-hook concurrency or cancellation.
- Polling sleeps are acceptable backoff while observing a predicate; fixed sleeps must never establish readiness, overlap, or cleanup success.

## Evident Push Sites Outside Common Progress Runner
- **Direct synchronous spawn:** `src/git/branch.rs:18–34`, `delete_remote_branch`: `std_git_command().args(["push", remote, "--delete", branch]).output()`. Not covered by progress-runner-only isolation; keeps narrow rewrite override.
- **Other owned async runners:** stale-branch remote deletion (`src/git/mod.rs:5187–5195`), Nix PR push (`src/nix.rs:228–236`), tag push (`src/release.rs:72–83`) use `run_git_with_timeout_env`; upstream repair (`src/report.rs:7938–7944`) uses `run_git_with_timeout`.
- **Synchronous capture:** concern diagnosis invokes `run_git_capture_output(... ["push", "--dry-run", ...])` (`src/report.rs:8432–8437`). A dry-run is not a remote update, but still merits hook/workload scope review.
- Search was literal `"push"` within `src`, following requested Git-source inspection; not an exhaustive audit of constructed commands, scripts, or external tools.

## Start Here
Open `src/git/push.rs:773–864` first: it already provides the exact real-Git bare-remote enforcement foundation. Add synchronization/concurrency coverage at the production admission boundary the parent identifies, and cancellation coverage next to the owned runner fixtures.

## BLOCKERS
- No blocker to fixture design. Production lane independence remains unproven until exercised through actual daemon admission/commit wiring; no implementation or tests were run here.
- Do not satisfy isolation by disabling hooks, setting rewrite override on normal pushes, capping memory, or adding blanket service limits. A progress-runner-only change leaves the explicit bypass paths above outside its scope.
