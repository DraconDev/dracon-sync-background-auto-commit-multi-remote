# Stale pinned publication target — re-gated for hours (2026-10-29)

## Observation (read-only, direct process + gate evidence)

`browser-extensions-shared` was blocked for hours while its repairable gate
failure was **already fixed in `main`**. The blocker was the *object being
published*, not the code:

| when | pre-push gate started | pinned object | gate result |
|---|---|---|---|
| 16:03:02Z | `dracon-pre-push.yX25xv` | `10671395b5` | cancelled |
| 17:33:01Z | `dracon-pre-push.xNjpDK` | `10671395b5` | failed (markdown-reader LINT) |
| 18:20:03Z | `dracon-pre-push.LyqohC` | `10671395b5` | failed (markdown-reader LINT) |
| 18:43:33Z | `dracon-pre-push.624gNQ` | `10671395b5` | running at capture |

Every one of those gates evaluated the **same immutable commit**,
`10671395b5b37308222308f939f20dfed7a20abd` (committed 16:02:00Z). Meanwhile the
actual branch advanced far past it:

```
10671395b5  2026-10-29 16:02:00 +0000   ← every gate run pinned this
13041f4de0  2026-10-29 18:39:57 +0000   ← the markdown-reader lint repair
HEAD        18:55:15Z                    (340 unpushed commits at 18:55)
```

The lint defect the gates kept reporting (`lib/markdown/plugins/inline-marks.ts`
et al.) is fixed in `13041f4de0` and at `HEAD`
(`/^([^^\n]+)\^/`, `/^\+\+([^+\n]+)\+\+/`), but the pinned pre-fix tree still
contains the original over-escaped classes:

```
$ git show 10671395b5:.../inline-marks.ts   → pattern: /^\^([^\^\n]+)\^/
$ git show HEAD:.../inline-marks.ts         → pattern: /^\^([^^\n]+)\^/
```

Direct process evidence at 18:56Z (read-only `/proc` inspection):

```
1099029  ppid 1642878  elapsed 13:02  git push github 10671395b5b37308222308f939f20dfed7a20abd:refs/heads/main
1099965  ppid 1099432  elapsed 11:59  node .../scripts/git-hooks/run-push-gates.mjs
```

The refspec is the fully-qualified pinned form
(`<object-id>:refs/heads/main`), which only `PublicationTarget::refspec()`
produces — so this is the daemon's own pinned publication round, not an
operator or agent push. The scheduler stamp agreed:

```
/home/dracon/Dev/browser-extensions-shared  phase=pushing
  execution_secs=10502  publication_secs=10488  publication_phase=pushing
```

`publication_secs=10488` is 2 h 55 m of continuously running publication,
matching a round pinned at ~16:02.

## Why this is a real convergence defect, not bad luck

`src/sync/publication.rs::pin_current_tip` resolves `HEAD^{commit}` **once per
round**, deliberately (`PIN LAST`, "still cannot move afterwards: the upload
holds no local mutation rights"). That design protects an in-flight upload from
publishing a tip that moved underneath it — correct for the *duration of one
round*. The runaway was **within** one round:

1. The pre-push hook re-validates the **pinned** revision's tree
   (`snapshot-<sha>`), which is exactly right — the bytes being published must
   pass.
2. A gate failure is a **local hook rejection**, which correctly **fails fast**
   in `push_with_retries_for_target` (`is_local_hook_rejection` → return before
   the retry/pull/HTTPS budget). So the *origin* leg ended after one gate run.
3. `upload_publication` then ran the **mirror legs regardless**: github and
   gitlab each invoked the same hook over the same immutable tree. Every leg
   paid a full 44-workspace 4-gate (~12 minutes, 159 gate logs) to re-learn a
   verdict already decided, and `push_retries = 3` multiplied it per leg.

The timeline confirms it stayed inside one round: the round began at 16:02:56Z
(it pinned `10671395b5`, committed 16:02:00Z) and ran until ~19:17Z — about
**3 h 15 m**, matching the scheduler's `publication_secs=10488` (2 h 54 m) at
18:57Z — with four full gate manifests started inside it (`yX25xv` 16:03Z,
`xNjpDK` 17:33Z, `LyqohC` 18:20Z, `624gNQ` 18:43Z). It ended by exhausting its
retry budget, not because of this repair (which is not deployed).

So the round was not "publishing stale bytes forever" — it was spending hours
re-running an expensive, deterministic hook on bytes that provably could not
change. Confirmed by the negative control below: removing only the local-hook
fail-fast arm makes a single pinned refusal execute the hook **five times**
(`xxxxx`) instead of once, and takes 10.67 s instead of 0.07 s in a fixture
whose hook is instant. With a real 12-minute gate, five executions is an hour;
with three mirrors plus retries it is the observed ~3 hours.

## Repair

`upload_publication` (`src/sync.rs`) now treats a local pre-push hook refusal of
the pinned object as a **decided verdict for that object**:

- The origin leg records the failure as before (the ledger still learns WHY),
  and additionally sets `origin_hook_refused`.
- The mirror legs are **skipped** for that round, with an explicit log naming
the pin and the reason. The hook gates worktree bytes, not destinations, so a
mirror cannot learn anything origin did not.
- The round returns `PushReport::Attempted { ok: false, degraded: false }`, so
the normal failure path still records the stuck-ledger entry, notification and
backoff. Nothing is reported as success and nothing is hidden.

Ending the round returns control to the scheduler, so the **next** round pins
the current tip — which is what lets a repair committed behind a stale pin ever
publish. The per-round pin guarantee is unchanged: no upload ever publishes a
moving HEAD.

Behaviour deliberately preserved: non-hook origin failures (transport,
rejection, timeout) still run the mirror legs, because those are per-destination
facts and a healthy mirror must still receive the bytes. Only a **local hook**
verdict — which is a fact about the pinned tree — short-circuits the fan-out.

## Regression coverage

`src/git/push_publication_tests.rs::local_hook_refusal_is_a_decided_verdict_for_a_pinned_object`
(`#[cfg(unix)]`, real Git, empty-hook-safe fixture):

- a hook that refuses the way a product gate does and appends one byte per
  invocation;
- a deliberately large retry budget (`retries = 5`) to prove the rule, not the
  budget, is what bounds execution;
- assertions: the refusal stays classified as a local hook decision, the hook
  ran **exactly once**, nothing reached the remote, and the local tip is
  unchanged.

Disposable negative control (isolated edit, restored afterwards): removing only
the local-hook fail-fast arm from `push_with_retries_for_target` makes this
test fail with `left: "xxxxx" right: "x"` — the hook re-ran five times on one
pinned object. The assertion therefore detects the real regression rather than
restating the implementation.


## What this report does and does not establish

Established by direct observation:

- The pinned object was `10671395b5` in four separate gate runs spanning
  16:03Z–18:43Z, and `HEAD` is 340 commits ahead of the remote tip.
- The gate failure reported by all four runs is genuinely fixed at `HEAD`.
- The publishing process is the daemon's own pinned round (`/proc` refspec +
  scheduler stamp, `publication_secs=10488`).
- Per-leg re-execution of the hook is the amplification: the negative control
  shows 5 executions where 1 is correct, on identical pinned bytes.
- Publication had therefore not advanced for that repository for ~3 h 15 m, all
  inside a single pinned round.
- The round ended on its own (retry budget exhausted) and a later round pinned
  the then-current tip `cf17489b33`. That is consistent with the repaired
  behaviour but is NOT evidence for it, because the repair is undeployed.

NOT established here, and deliberately not claimed:

- That the markdown-reader repair has been *published*. It is committed locally;
  the pinned round predates it.
- That any repository other than `browser-extensions-shared` is affected in this
  window.
- That this is the only remaining cause of that repository's delay. The repair
  bounds re-execution of a *decided* verdict; it does not make a genuine gate
  failure pass, and it does not shorten a single legitimately slow gate run.


## Repair status

The **content** repair is complete and verified in the live worktree
(`checks/browser-markdown-reader-lint-2026-10-29/REPORT.md`: lint exit 0 with 0
errors, typecheck exit 0, 61/61 tests, plus disposable controls proving the new
assertions accept the behaviour-preserving cleanup and reject the two plausible
wrong fixes).

The **amplification** repair is implemented with a regression test and a
disposable negative control (above). It still requires the full workspace gates
and qualified deployment before the daemon runs it.

Making the markdown-reader repair *publish* additionally requires the daemon to
stop re-running the decided gate and to pin a newer tip. Because publication is
the daemon's alone (no manual push is permitted to manufacture convergence),
the correct next observation is whether a round started after a deployed fix
pins the repaired tip. That is a live observation, not something this report can
assert in advance.

