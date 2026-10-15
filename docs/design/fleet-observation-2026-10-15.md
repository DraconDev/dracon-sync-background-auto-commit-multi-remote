# Fleet observation — 30-minute post-deployment window (2026-10-15)

## Window and ground rules

- Window (UTC): T0 2026-10-15T23:06:03Z → Tend 2026-10-15T23:37:46Z
  (~31.7 minutes, covering 6+ 300s retry cycles). Mid-window samples at
  T+10 (23:16:45Z) and T+20 (23:27:17Z).
- No rollout changes were made during the window: same daemon PID 3440303
  at T0 and Tend, running-executable SHA-256 `b8621cee…c90115a`
  (the deployed build) at both ends, no `maintenance` invocation, no unit
  edits, no freeze marker present before, during, or after.
- Raw snapshots: `evidence/fleet-observation-2026-10-15/repos-T0.txt`,
  `repos-T10.txt`, `repos-T20.txt`, `repos-Tend.txt` (+ `parsed.json`).

## Fleet state: beginning → end

34 repos at every snapshot. Summary line:

| Snapshot | Clean | Active | Queued | Failed | Blocked |
|---|---|---|---|---|---|
| T0 | 24 | 9 | 1 | 0 | 0 |
| T+10 | 19 | 13 | 2 | 0 | 0 |
| T+20 | 18 | 15 | 1 | 0 | 0 |
| Tend | 19 | 15 | 0 (1 QUEUED*) | 0 | 0 |

Zero ❌/⛔ rows at all four snapshots: no STALLED/STUCK state was displayed
at any sample. (*`dracon-platform` shows 🟡 QUEUED at Tend; see cause 3.)

Movement during the window (status flips T0→Tend): `deathrun`,
`endless-td`, `neonbreak`, `hellhunter`, `doomtap` CLEAN→WORKING (live
game-loop activity, each with fresh push-OK afterwards); `football-forever`
WORKING→CLEAN (converged, push OK 20m); `polis` WORKING→CLEAN→WORKING→WORKING
with ahead 1→0→1→0 (converging through retries); `pi-plugins` and
`pi-goal-list-loop-audit` WORKING→CLEAN (both push OK ~11–12m at Tend);
`.dracon` UNKNOWN→WORKING (still push-blocked, cause 1); `db-gateway`
CLEAN→WORKING with push OK 31m (normal activity). `browser-extensions-shared`
stayed WORKING throughout with ahead growing 19→21→25→31.

## Retry cycles observed (repeated, in-window)

- `.dracon` stuck-push retry after 301/302/304s with alert (00:06 local),
  on backoff after the pre-push refusal — the 300s retry loop firing as
  designed, fail-closed.
- `browser-extensions-shared` push retry 3/3 twice (00:06, 00:25 local),
  with push-timeout scaling 900s→1800s→3600s as ahead grew 19→20→25.
- `polis` pre-window Dispatch-Starved alert followed by in-window pushes
  (OK 15m, 26m) and ahead returning to 0 — alert→retry→recovery.
- `pi-goal-list-loop-audit` pre-window status-spawn failure → 300s retry →
  CLEAN at Tend. Retry loop firing as designed.
- Daemon's own index.lock hygiene: stale lock in `dracon-platform` found
  fuser-clean and swept (00:07 local), no manual intervention.
- `dracon-sync health` healthy (daemon running, freeze off, policy valid)
  at all four samples; journal shows ~556 commit/sync lines in-window
  across 15+ repos (dracon-strategy ×38, platform ×26, capture-anime-girls
  ×24+17, junk-runner ×20+8, neonbreak, darklord, polis, hellhunter,
  endless-td, deathrun, freeport, pi-plugins, utilities/dracon-sync, .dracon).

## Scheduler starvation / orphan-work assessment

No fleet-wide scheduler starvation: while two per-repo Dispatch-Starved
alerts fired (see causes), the rest of the fleet committed and synced
continuously through the window. No orphan-work accumulation: no freeze
marker, no `.dracon-sync.new` leftover (atomic rename completed at deploy
time), no STALLED/STUCK rows, and the only stale lock found was swept by
the daemon itself. `~/.dracon/backups/` holds exactly one file: the
rollback copy (cause 1).

## Remaining pending causes (every non-converged item accounted for)

1. **`.dracon` push FAIL (OPEN, self-inflicted, fix identified).**
   The pre-push secret scan refuses
   `backups/dracon-sync.rollback-20261015-pre-covers-deploy` — the rollback
   binary preserved by the deployment goal trips the binary-safe scan, so
   every push fails closed and backs off. Hook behavior is correct; the
   placement was wrong. Next: relocate the rollback copy outside the watched
   `~/.dracon` tree (e.g. under `~/.local/state/dracon/backups/`) as a
   follow-up goal, then confirm `.dracon` pushes resume. Not fixed here:
   this goal forbids rollout changes.
2. **`browser-extensions-shared`, 31 ahead (OPEN, live-generator backlog,
   not failed convergence).** Ahead grew 19→31 during the window while the
   daemon committed 13- and 8-file batches into it — the generator produces
   faster than the giant push drains. Pushes are attempted with scaled
   timeouts (up to 3600s), never refused by hooks; Stuck-Ahead and
   Dispatch-Starved (6101s) alerts plus the >300s-unpushed backstop (which
   pauses auto-commit for that repo only) are the designed pressure valves,
   and other repos are unaffected. Distinction: backlog from continuous
   generation, not a systemic push failure — but the lane is effectively
   wedged behind one enormous push, so it stays open pending a drain
   strategy (smaller push batches or pausing the generator).
3. **`dracon-platform` QUEUED at Tend (WATCH, transient).**
   Its sync task was wedge-cancelled 3 minutes before Tend (after a 666s
   dispatch-starved hold); classification succeeds (~60s filter-aware diffs
   at 00:09/00:19 local), a stale index.lock was swept, push OK 19m.
   Expected next step is redispatch on the following cycle — not a failure,
   but confirm it leaves QUEUED in the next observation.
4. **Small in-flight items (normal).** `darklord`, `dracon-strategy`,
   `capture-anime-girls` each 1 ahead with fresh push-OK (1–12m);
   `junk-runner`, `deathrun`, `endless-td`, `doomtap`, `neonbreak`,
   `hellhunter`, `polis`, `dracon-sync`, `db-gateway` WORKING with current
   activity — all consistent with ordinary commit→push latency, none
   alerting, none aging.

## Backlog vs convergence (explicit)

Converged or converging in-window: `football-forever`, `polis`,
`pi-plugins`, `pi-goal-list-loop-audit`, plus steady commit/sync flow in
15+ repos. Normal live-generator backlog: `browser-extensions-shared`
(cause 2) — generation outpaces a single giant push; the daemon contains
it per-repo without fleet impact. Failed convergence: none fleet-wide;
the two open items are cause 1 (blocked by a file this operator placed —
fix queued) and cause 2 (backlog drain strategy needed). No systemic
failure is claimed resolved that is not; both stay open.
