## Review — independent publication (read-only rehearsal)

Scope: full working tree at HEAD (evidence dir + `src/{sync.rs,sync/publication.rs,git/push_resources.rs,git/sync_lease.rs,git/push.rs,git/multi_remote.rs,daemon.rs,daemon/scheduler_evidence.rs,current_health.rs,report.rs}`). Method: source reading + receipts; no test run this session. **NO P0 FOUND.**

**Correct**
- One-publisher invariant is kernel-backed and identity-canonical: `sync_lease::repository_identity` canonicalizes the *common* dir (`git/sync_lease.rs:22-31`), flock on a persistent file with `O_NOFOLLOW|O_CLOEXEC`, regular-file + owner + `nlink==1` checks (`:44-79`); alias/worktree/cross-process exclusion is covered by real subprocess tests (`:126-192`). In-process cross-repo concurrency is proven by `sync/publication_actor_tests.rs:168-283` and the isolated receipt (`isolated/receipt.json`: A blocked, B published, A's second commit survived, remote converged on newest tip).
- Pinning is immutable and cannot retarget: `PublicationTarget` rejects `HEAD`/`0…0`/unsafe refs (`git/push.rs:362-395`), every origin retry and HTTPS fallback names `object_id:refs/heads/branch` (`push.rs:437-441,600-615`), pinned rejections fail fast with no pull/merge (`push.rs:496-500`, `multi_remote.rs:757-760,802-806`), verified by `git/push_publication_tests.rs:45-140`.
- `PublicationPending` is never success/failure: mapped to `PushPaused` retaining activity/blocked/ledger (`daemon.rs:1124-1129`), and gated so a real `PushFailed`/`Blocked` still wins (`sync.rs:5180-5193`).
- Pinned transfer no longer holds local capacity (`push_resources.rs:47-79`) and `local_phase` re-admits with its own permit while keeping the outer scope yielded (`:84-100`), pinned by `push_resources_tests.rs:58-158`.

**Finding: P1 — refresh rounds stop after 32 passes, so a long hook re-freezes commits**
`sync.rs:2318-2324` (`PUBLICATION_MAX_REFRESH_ROUNDS = 32`), sleep arm guard `sync.rs:2408`. After 32 rounds (~24 s active / ~96 s idle at `PUBLICATION_REFRESH_IDLE=3s`) no further local pass is scheduled, and the daemon cannot redispatch because the repo stays `in_flight` (`daemon.rs:9700-9712`). Scenario: a 15-minute pre-push hook (documented in AGENTS.md as observed: 15 m 0.5 s) again blocks new commits for the remaining ~13 minutes — the exact symptom this change removes; the live receipt only exercised ~100 s.

**Finding: P1 — a cancelled/interrupted mirror round wipes per-remote pause memory**
`upload_publication` takes the map with `std::mem::take` (`sync.rs:2555`), restores it on origin cancellation (`:2567-2569`) but not on the interrupted path of `aggregate_push_results(...)?` (`:2596`, `?` propagates before `plan.remote_failures = mirror_map…` at `:2600`). `run_publication_actor` then clones the emptied plan map into `ctx.remote_failures` (`sync.rs:2453-2456`), so `apply_outcome` overwrites `entry.remote_failures` with `{}` (`daemon.rs:1034-1037`) — the R4-SC-02 regression shape, reachable via mirror cancellation. Smallest fix: restore `plan.remote_failures` in a `?`-safe way (compute into `map`, restore, then convert Err).

**Finding: P1 — failed post-upload lease reacquisition still mutates locally**
`acquire_local_lease` returns `None` on timeout/foreign owner (`sync.rs:2480-2500`) but `run_publication_actor` assigns it without checking (`:2476`), and the caller continues into local mutation afterwards (`run_release_pipeline_if_bumped` at `:4778`, `maybe_sync_visibility_and_metadata` at `:5890`). Scenario: a CLI `sync-now` holds the operation lease; the daemon's post-upload bookkeeping writes metadata/tags without the lease, i.e. concurrent local mutation the lease exists to prevent. Smallest fix: propagate `None` as a deferral (skip that tail) rather than continue.

**Finding: P1 — PUBLISHING disappears after 120 s, so long blocked uploads render WORKING/QUEUED/STALLED**
`scheduler_evidence.rs:186-190` drops any stamp whose `touched` is older than 120 s, and `publish_phase` stamps once (`:74-88`) with no refresh during the upload. Live evidence shows `publication_secs 99` (`live-observation.md`) — just inside the window. At >120 s the row falls back to recovery `Working`, then `Queued`, then (≥900 s) `Stalled` (`current_health.rs:65-95`). So the truthful-phase guarantee holds only for sub-2-minute uploads, not for the multi-minute hooks the change targets. Fix: refresh `touched` from a progress tick or widen the window for an executing publication (careful: the expiry exists to kill abandoned stamps, so prefer a bounded refresh tied to the live future).

**Finding: P1 — a second publisher can still run for one repo outside `sync_repo`**
`report.rs` repair paths push without any lease: `push_with_retries` (`:8168`, `:8241`, `:8557`), mirror push (`:8194`, `:8589`), `force_push_after_rewrite` (`:8432`, `:8476`); `daemon.rs:8520` `background_pushes` default-branch rename. `grep` shows zero `sync_lease::try_acquire*` in `report.rs`. With the global push semaphore removed, `dracon-sync repair concerns --apply` can now upload `HEAD` concurrently with the daemon's pinned round for the same common dir, contradicting `sync_lease.rs:36-38`. (Pre-existing gap, newly reachable because serialization is gone.) Smallest fix: take `try_acquire_publication` non-blockingly in `handle_ahead`/mirror arms and skip with a deferral line.

**Finding: P1 — push concurrency is now bounded only by repo count**
After the global gate, the only remaining push bounds are: the local semaphore (`sem_max_concurrent_sync`, default 4, `policy.rs:1327-1329`, `daemon.rs:7221`) which the upload *yields* (`push_resources.rs:60-63`), the per-repo publication lease, operation deadlines, nice 19 and process groups. Per repo: origin + mirrors in parallel (`multi_remote.rs:974-1000`), each with `push_op_timeout_secs` (operator 3600 s) × `push_retries`, plus a 600 s janitor `push --delete` (`git/mod.rs:5203`) and tag/nix pushes. A fleet-wide forge stall can therefore reach ~repos × remotes concurrent `git push` children (36 × 4 ≈ 144), bounded only by `TasksMax=512`; on exhaustion spawn failures hit unrelated work (commit hooks), not just pushes. Not a correctness bug — a capacity decision that should be stated explicitly, with a per-daemon push cap or a per-forge cap if the intent was "one slow forge must not starve the box".

**Finding: P2 — release/tag path is dead code**
`let version_bumped = false;` (`sync.rs:4471`) is the only value ever passed to `run_release_pipeline_if_bumped` (`:4778`), and no `bump::` caller exists; `ctx.auto_bump_versions` is `#[allow(dead_code)]` (`:310-311`). So `auto_tag`/`auto_release` never fire, and progress.md item "release identity" is moot rather than pending. Confirm whether this predates the change; if not, it is a silent feature loss.

**Finding: P2 — duplicated dead block**
`sync.rs:2519-2524`: the `if commit_only { return AllPaused }` guard is written twice; remove one.

**Verified non-issues**
- `tokio::select!` (`sync.rs:2397-2440`): no starvation/deadlock — the publication branch is always enabled, the in-flight refresh is never cancelled (awaited at `:2443-2455`), and the `else` arm is unreachable while an upload is pending. Cancellation of the whole worker drops the upload future (killing the push child via `kill_on_drop` + process group) before `ctx` releases the publication lease — verified by read order, not by test.
- `local_phase` nesting is safe: nested admission reuses the existing permit (`push_resources.rs:86-90`), pinned by `push_resources_tests.rs:58-90`.
- Daemon commit-hook policy preserved: `commit_staged_owned` uses `git commit --no-verify` in a bounded owned call (`sync.rs:5079-5087`), with in-process bucket/storage/exclusion/size gates still ahead of it (`:4556-4600`); witnessed failing-fixture regression noted in progress.md.
- Evidence receipts check out as far as they go: `workspace-suite.txt` has no `FAILED`/`error[` lines (1390 + 551 + 220 … all `test result: ok`), and `installed-receipt.json` hashes the *loaded* `/proc/<pid>/exe`, not just metadata.

Merge verdict: **OK with notes**

BLOCKERS: none (no P0). The six P1s above should be fixed before the next release tag; the 32-round refresh cap and the 120 s stamp expiry are the two that directly undermine the stated contract.