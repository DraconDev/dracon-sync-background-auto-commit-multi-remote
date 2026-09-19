# Watchdog audit: every scheduler structure that can suppress a repo (2026-09-18)

Goal context: full-program P0 item 3. Method: enumerated every map/set in
`dracon-sync/src/daemon.rs` whose membership can skip, delay, or pause a
repo, and verified each has a release bound + a log line. Two minor leaks
fixed (v0.113.68); one earlier claim corrected.

## Inventory

| Structure | Suppresses via | Bound (release) | Log / signal | Verdict |
|---|---|---|---|---|
| `in_flight: HashSet` | no-redispatch skip | join removes; 15-min wedge → cancel request, ownership until teardown | `task_start`, wedged warning | ✅ bounded (residual disclosed: trailing-drain abort holds until restart) |
| `detached_syncs` + `detached_since` | results applied once; wedge detection | 15-min `should_discard_stale_detached_result` + cancel request | trailing-drain + wedged warning | ✅ bounded |
| `sync_workers: HashMap<AbortHandle>` | n/a (ownership, not suppression) | removed on both join paths | — | ✅ no leak |
| `classification_pending` + `_since` | spawn gate (no result → no dispatch) | 30s job timeout + 90s pending watchdog + re-probe | `classification_job_done`, watchdog warn | ✅ bounded (v0.113.65) |
| `classification_cooldowns` | spawn gate | 500ms–1s, then re-probe | skip reason | ✅ bounded |
| `remote_notify_streaks` | n/a (notification backoff input) | keys mirror `remote_notify_cooldowns` 1:1; cleared conditions remove keys | — | ✅ bounded (v0.113.77 escalating throttle: 30m→8h cap) |
| `classification_results` (+taken_at stamp) | n/a (authorizes dispatch) | consumed at dispatch; staleness pass drops empty-dirty/failures AND non-empty results past 120s max-age on dirty repos (`classification_stale_refresh` + 500ms re-probe) | stale-refresh line | ✅ bounded (v0.113.76 fixed the stale-result pin: kept 815-entry snapshot filter-clean-skipped real dirt 30+ min) |
| `dispatch_holds` | n/a (observability) | pruned to live `activity` every cycle | snapshot file | ✅ self-cleaning (v0.113.67) |
| `last_dispatch` | n/a (starvation alert input) | pruned to live `activity` every cycle | — | ✅ self-cleaning (v0.113.67) |
| `quiet_evidence` | quiet-clock anchor | pruned to live `activity` every cycle | — | ✅ fixed v0.113.68 (was clean-path-only) |
| `max_fail_cooldowns` | 15-min re-probe backoff after 5 failures | 15-min `until`; expired pruned every cycle | maxfail warning + notify | ✅ fixed v0.113.68 (expired prune) |
| stuck ledger (`StuckRepoEntry`) | `Backoff` skip; `Exhausted` full pause | Backoff: 300s; Exhausted: **indefinite until operator `stuck-unstuck` or push success** | table STUCK + alerts | ✅ by design (needs-human); transient-infra no longer burns budget (v0.113.65/66) |
| `stage_cooldowns` | stage re-attempt delay | 60s/300s `until`, consumed on read | — | ✅ bounded |
| `mirror_consecutive_fails` | `Mirror Degraded` alert only (never skips) | reset on success (helper-owned write) | alert with classified cause | ✅ non-suppressing |
| `provisioning_jobs` | unfinished-job skip | jobs always joined or awaited in-loop; failure logged | forge provisioning warning | ✅ bounded |
| `ownership` (per-activity) | unowned skip | redetect TTL for Unowned/Unknown; dies with activity | unowned alert | ✅ bounded |
| freeze marker | whole-daemon pause | 1h hard TTL + 30m watchdog auto-clear | freeze warnings | ✅ bounded |
| `remote_notify_cooldowns` | alert throttle only | cleared on SIGHUP; keys bounded by repos×alerts | — | ✅ benign |
| `empty_bootstrap/auto_create/ls_remote_cooldowns`, `pending_repos`, forge cache | bootstrap pacing | cleared on SIGHUP; per-op TTLs | — | ✅ bounded |
| forge health (`dracon-sync-forge-health.json`, v0.113.73) | incident re-probe stretch (15m→60m); alert coalescing; stuck-budget shield | hits pruned to 10-min window, 50/host cap; incident clears on quiet window via per-cycle `poll_forge_recovery` | 🔥 declare / ✅ recover / 🛡️ shield lines | ✅ bounded (persisted SUPPRESSOR: survives restarts by design — that is the fix for restart amnesia; staleness bounded by the window) |
| `commit_only_repos` (v0.113.69–72) | Backoff/Exhausted dispatch commit-only; clean+commit-only skips dispatch (v0.113.70 throttle) | cleared on Retry/unstuck/success; retry stamp at dispatch (v0.113.71); degraded-PushPaused retains (v0.113.72) | commit-only + throttle skip lines | ✅ bounded (no timer: membership follows the stuck ledger) |
| status pipeline (`status_jobs`/`status_pending`/`status_spawned_at`/`status_results`, v0.113.74) | repo inspection waits for its own status only; others scan on | at most one task per repo (spawn gated on !pending); results >30s old dropped + re-probed; failures keep old skip semantics; pending released on collect | `status-pending` hold + `status-stale` re-probe lines | ✅ bounded (no timer: ownership ends at task completion; staleness cap 30s) |
| `pile_watch` (v0.113.74) | n/a (observability: arrival/drain windows) | pruned to live `activity` every cycle; window resets on alert | `Pile Growing` alert with rates | ✅ self-cleaning |
| classification backoff (v0.113.74) | re-probe cooldown scales 1s→5min with consecutive failures | cap 300s; success resets via collect; 90s pending watchdog unchanged | scaled `re-probe in Ns` line | ✅ bounded |

## Record correction

The 2026-09-18 program proposal message claimed stuck entries have a
"24h expiry". **No such expiry exists in source** (no 86400/expiry path;
verified by grep). Exhausted persists until operator unstick or a
successful push clears it. This is by design (needs-human must not
self-clear silently), not a gap. The proposal text is superseded by
this doc.

## Changes (v0.113.68)

- `prune_repo_liveness(&mut quiet_evidence, &activity)` at persist.
- `prune_expired_cooldowns` helper + per-cycle call for
  `max_fail_cooldowns` + boundary test.
