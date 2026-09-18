# P2 commit/push decoupling — verdict: UNNECESSARY (2026-09-18)

Trigger condition (goal `20260918134807-iw7crj`): ship P2 ONLY if (1)
commit-despite-paused-push proves insufficient on 500MB-pack repos, with
live measurements quoted. Otherwise close as unnecessary.

## Live measurements (dracon-sync 0.113.74, PID 2854467, same-domain Unix-ms)

500MB-pack repos present: `ai-auto-writer` (547MB + 508MB packs, 501
untracked firehose pile, 166-ahead push backlog), `dracon-platform`
(503MB pack, clean parent).

1. **Small-repo probe beside churning giant** (`warden-vid`, 504K .git,
   while ai-auto-writer staged 551-file batches): file→commit
   **69,115ms** = **67,737ms** inspection wait (status task exceeded the
   30s staleness bound → `status-stale` re-probe → collected; journal:
   `eligibility ... observed_quiet_ms=67737 ... dispatch ...
   unix_ms=1789761440481`) + **~1,378ms** worker commit.
2. **Giant self-probe** (ai-auto-writer): file→commit **220,131ms**
   (~3.7 min), riding the normal batch cycle (workers landing 100+
   files/commit every ~2–3 min: `7ffb6f901d` 104 files, `dacd41ead4`
   101 files; diff 10.6s + stage/add ~6s per cycle).
3. **Giant inspection healthy**: ai-auto-writer status `repo_ms`
   1521/1690/2983ms — no inspection starvation on the giant itself.
4. **No push-blocked commits anywhere**: stuck ledger
   `dracon-sync-stuck-push-repos.json` empty, zero push-paused repos —
   the (1) commit-only path is not even engaged; backlog drains via the
   normal commit→push path.

## Analysis

Full commit/push decoupling (independent commit vs push schedules)
would shorten NEITHER measured dominant term:

- The 69s small-repo delay is entirely **pre-commit inspection**
  (status queue + 30s-stale re-probe). Push is not in the path: the
  commit landed and push follows asynchronously without gating the
  next commit.
- The 220s giant delay is **firehose batch cadence** (generator
  outruns drain); the commit phase itself is healthy (100+/commit).
  Decoupling push would not accelerate staging or committing.
- Worker phase measured fast (1.4s); no evidence of push blocking
  commits on any repo, giant or small.

## Verdict

**(1) is sufficient on 500MB-pack repos. P2 CLOSED AS UNNECESSARY.**

Recorded follow-up (NOT P2, NOT this goal's scope): the 67.7s
inspection wait + ~31 fleet-wide `status-stale` trips/10 min under
host load ~50 show inspection-side contention that a future
status-lane reservation could address. The pipeline's 30s staleness
guard + re-probe already bounds it; no wedge, no starvation observed
(both probes committed).
