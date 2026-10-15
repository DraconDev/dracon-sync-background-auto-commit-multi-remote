## Review

- **Correct:** Main daemon workers enroll their existing local-work permit (`src/daemon.rs:9314–9333`). `push_phase` releases it and reacquires it on success or error before returning (`src/git/push_resources.rs:46–74`).
- **Correct:** Mirror futures remain owned, concurrent futures—not detached tasks—so task-local isolation propagates (`src/git/multi_remote.rs:885–912`). Nested phases do not restore the permit prematurely.
- **Correct:** Explicit field order drops command work before releasing its push slot (`src/git/push_resources.rs:109–118`). Cancellation reaches the owned process-group kill guard (`src/git/ops.rs:144–176`).
- **Correct:** Nice 19 applies only to enrolled push children; no memory caps, service-limit increases, or hook bypasses were introduced (`src/git/push_resources.rs:120–138`).
- **Correct:** Default-branch repair now runs in background maintenance with per-repo exclusion and retry cooldown (`src/daemon.rs:7468–7485,8218–8247`).
- **Correct:** Real-hook tests inspect and commit another repository before releasing the slow hook; rejection leaves the bare ref unchanged. Cancellation verifies hook/helper termination and continued interactive-sentinel responses (`src/sync/push_isolation_tests.rs:179–317`).

### Finding: P1 — Daemon push entry points escape isolation

**Locations:** `src/sync.rs:4782,5199–5204`; isolation predicate at `src/git/push_resources.rs:84–85`.

Only commands inside `PUSH_PHASE` receive the separate lane and child priority. Two daemon-worker paths still push outside that phase:

1. **Automatic release tags:** After normal pushing has restored the local-work permit, `run_release_pipeline_if_bumped` invokes the release pipeline (`src/sync.rs:2005–2028,4782`). With `auto_tag`, this reaches an unwrapped tag push (`src/release.rs:72–85,656–658`).
2. **Stale-branch janitor:** Before normal pushing, the opted-in janitor reaches an unwrapped remote deletion (`src/sync.rs:5199–5204`; `src/git/mod.rs:5188–5197`).

Both retain local-work capacity while executing potentially heavy pre-push hooks, bypass the single-command lane, and omit nice 19.

**Source-derived repro:** Set local concurrency to one. Block a real pre-push hook specifically on an automatic tag push—or an eligible janitor deletion—then queue another repository’s commit. That worker cannot acquire local capacity until the hook releases. The supplied fixture exercises `push_background` directly, so it does not cover these paths.

**Smallest fix:** Wrap each actual push operation in `push_phase`, preserving enclosing local mutations and interactive behavior. Add a slow-hook regression through these daemon entry points.

These call paths predate the diff; the finding is **incomplete enrollment under the new contract**, not newly introduced hook bypass or historical enforcement breakage. Unenrolled CLI/interactive operations are intentionally excluded.

### Validation attribution

Independent work was read-only source/artifact inspection. **Parent-provided** `focused-final.log` records passing 2 resource, 2 real-hook, 1 existing-enforcement, and 6 branch tests. Parent-provided `clippy.log` records successful completion; command exit 0 was supplied by the parent. I did not execute tests or install/deploy anything.

**Merge verdict: BLOCK** — covered paths satisfy the core fixture, but daemon-worker bypasses retain the starvation mechanism.

## BLOCKERS

- Enroll automatic tag pushes and stale-branch remote deletions in scoped isolation; verify unrelated commit progress while their real hooks remain blocked.