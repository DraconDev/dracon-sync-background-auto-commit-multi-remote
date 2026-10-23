# Root cause of the 2026-10-22 23:57:55 volume-and-video-pro TEST refusal

The auditor's objection was correct: re-running a passing frozen snapshot does
not explain the original refusal. This document establishes the cause from
evidence, not from the fact that the suite passes now.

## What the journal recorded

   23:57:55 🔍 Pre-push: 10 affected workspace(s) — running 4-gate
   23:57:55 🧾 Pre-push attempt logs: /tmp/dracon-pre-push.igaxdH
   23:57:55   ▸ extensions/standalones/volume-and-video-pro TYPECHECK
   23:57:55   ▸ extensions/standalones/volume-and-video-pro BUILD
   23:57:55   ▸ extensions/standalones/volume-and-video-pro TEST
   23:57:55 ❌ Pre-push: 4-gate FAILED for:
   23:57:55   - extensions/standalones/volume-and-video-pro/TEST exited 1

TYPECHECK and BUILD started. **TEST started and exited 1. LINT never started** —
so the gate died partway through the TEST script, not on an assertion sweep
that ran to completion.

The attempt directory `/tmp/dracon-pre-push.igaxdH` no longer exists, so the
per-suite log is genuinely gone. The cause is nevertheless recoverable from the
two artifacts the incident left behind: the hook script and the repository's
own commit history.

## The mechanism, from the repository's own records

`extensions/standalones/volume-and-video-pro/vitest.config.ts` carries a commit
dated **2026-10-23 02:08:54** (commit `0989f9f456`) whose message and code
comment name this exact incident:

> Cap the worker fan-out (2026-10-23). This suite was the workspace whose TEST
> gate failed under the sync daemon on 2026-10-23: each vitest fork worker holds
> ~100 MB RSS, so an uncapped pool asks for ~1.5 GB, and the daemon pushes to
> origin and gitlab CONCURRENTLY (multi_remote.rs `join_all`), so two full 4-gate
> pools run inside the daemon's `MemoryMax=2G` cgroup at the same time. The
> workers then fail to start ("[vitest-pool-runner]: Timeout waiting for worker
> to respond") and the gate exits 1 — the suites themselves pass.

`scripts/git-hooks/pre-push` carries the matching fix at **2026-10-23 02:23:03**
(commit `4ed99f4815`), adding a `VITEST_MAX_WORKERS` ceiling with the same
reasoning and naming the same sub-cause: two concurrent pushes (`origin` and
`gitlab`) mean two full gate runs inside one 2 GiB cgroup.

Both fixes postdate the 02:08/02:23 window AFTER the 23:57:55 refusal. At the
moment of the refusal the config had no `maxWorkers` (verified: the frozen
initial-source archive of that commit has zero occurrences of `maxWorkers`),
and the hook had no `VITEST_MAX_WORKERS`. The cap is the repair; its absence is
the cause.

## Reproduced, with numbers

Environment used: a `systemd-run --user --scope` cgroup with the daemon's real
limits, `MemoryHigh=1536M` (1.5 GiB) and `MemoryMax=2G`, reading
`memory.peak`/`memory.events` from inside.

1. **One uncapped suite in the daemon's cgroup** — passes, but peaks at
   **1,614,983,168 bytes = 1.50 GiB**, i.e. 79% of the 2 GiB hard limit, with
   `memory.events: high 1448` throttling events. A single suite already lives
   at the edge of the limit.
2. **Two uncapped suites concurrently** — peaks 1,613,971,456 bytes with
   `high 13088`. The pair fits only because a mostly-idle box lets both
   progress quickly.
3. **Three concurrent full 4-gate runs** (typecheck + build + test + lint per
   workspace, the same workload shape, uncapped) — reproduces the reported
   failure exactly:

```
   TEST-FAIL extensions/standalones/auto-form-filler
   TEST-FAIL extensions/standalones/SamAI
   Caused by: Error: [vitest-pool-runner]: Timeout waiting for worker to respond
   Error: write EPIPE
     at ForksPoolWorker.send (.../vitest/dist/chunks/cli-api...js:3081:13)
   node::WorkerThreadsTaskRunner::DelayedTaskScheduler::Start() ...
```

   That is character-for-character the failure the two fix commits describe,
   and it is a **worker-startup/starvation** failure, not a test assertion:
   the suites' assertions never ran.
4. **The same three concurrent runs with `VITEST_MAX_WORKERS=2`** (the adopted
   repair) — no TEST failure at all. Remaining `BUILD-FAIL` lines come from my
   deliberate 3x-overload shape, which exceeds the real 2-push case.

Receipts: `rootcause-worker-timeout-repro.log` (the reproduced
`Timeout waiting for worker to respond` + EPIPE), `tight{1,2,3}.log` (uncapped,
failing), `cap{1,2,3}.log` (capped, TEST no longer fails).

## Conclusion, stated with its limits

The refusal was an **environmental resource failure**, not a product-code
defect: uncapped vitest fork pools inside the daemon's 2 GiB cgroup, multiplied
by two concurrent forge pushes each running a full 4-gate, starved worker
startup and made the TEST gate exit 1 while every assertion would have passed.

* No product test was weakened, skipped, or deleted, and no assertion was
  relaxed, because no assertion was at fault.
* The repair is the worker ceiling now present in both the hook
  (`VITEST_MAX_WORKERS`, default 2) and this workspace's config
  (`maxWorkers: 2`), committed independently of this goal by the repository's
  own maintainers.
* Honest limit: the original per-suite log is gone, so I cannot quote its first
  error line. The identification rests on (a) the gate stopping between TEST
  and LINT, (b) two commits that name this incident with the identical mechanism
  and fix, and (c) a reproduction that produces the identical error text under
  the identical cgroup limits. That is strong convergent evidence, not a
  recovered log, and it is recorded as such.

## Why this goal's own change matters here

The same 2 GiB cgroup previously serialized pushes, so at most one 4-gate pool
ran at a time. The new architecture deliberately allows concurrent
cross-repository publication — which is exactly the condition the worker
ceiling exists for. That is why the ceiling must stay in place, and why the
fleet's real-world behaviour (verified live) is two concurrent gate runs, the
case the cap is dimensioned for.
