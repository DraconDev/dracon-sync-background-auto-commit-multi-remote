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
round*.

The defect is what happens across rounds when the round's own hook is the
blocker:

1. The pre-push hook re-validates the **pinned** revision's tree
   (`snapshot-<sha>`), which is exactly right — the bytes being published must
   pass.
2. A gate failure is classified as a **local hook rejection**, which correctly
   **fails fast** (`is_local_hook_rejection` → return before the retry/pull/
   HTTPS budget). So the round ends without publishing.
3. A later round again pins a revision and re-runs the full multi-workspace
   gate. When the repair lands *behind* the pin, the freshly pinned revision is
   fine — but the observed sequence shows the same stale object being re-gated
   for ~2 h 40 m, so the pin is not being re-derived from the advanced branch in
   the intervening rounds.

The practical consequence is the worst of both worlds: the daemon is *not*
retrying a doomed action in the cheap sense (each attempt is a real
~12-minute, 44-workspace gate), and it is *not* publishing the fix either. The
gate cost compounds: at 18:43Z the run had already produced 88 gate logs, and
`dracon-pre-push.LyqohC` produced 159 gates. With `push_retries = 3` and three
remotes, one round can serialize many full gate suites over the same bytes.

This is the same *class* as the polis report's "correct guard, wrong state":
the mechanism is right, the state it acts on is stale.

## What this report does and does not establish

Established by direct observation:

- The pinned object was `10671395b5` in four separate gate runs spanning
  16:03Z–18:43Z, and `HEAD` is 340 commits ahead of the remote tip.
- The gate failure reported by all four runs is genuinely fixed at `HEAD`.
- The publishing process is the daemon's own pinned round (`/proc` refspec +
  scheduler stamp).
- Publication has therefore not advanced for that repository for ~3 hours.

NOT established here, and deliberately not claimed:

- The exact code path that re-derives the pin across rounds. `pin_current_tip`
  reads `HEAD` per round, so a single pathological site has not been isolated;
  candidate explanations are (a) one long-lived round whose retry legs each
  re-run the hook, and (b) a queue of rounds each planned before the repair
  landed. Distinguishing them requires instrumenting round start/end rather than
  inferring from stamps.
- That any repository other than `browser-extensions-shared` is affected in this
  window.
- Any claim that the markdown-reader repair has been *published*. It has not:
  it is committed locally and the pinned round predates it.

## Repair status

The **content** repair is complete and verified in the live worktree
(`checks/browser-markdown-reader-lint-2026-10-29/REPORT.md`: lint exit 0 with 0
errors, typecheck exit 0, 61/61 tests, plus disposable controls proving the new
assertions accept the behaviour-preserving cleanup and reject the two plausible
wrong fixes).

Making it *publish* requires the daemon to stop publishing the stale pinned
tree. Because publication is the daemon's alone (no manual push is permitted to
manufacture convergence), the correct next observation is whether a round
started after `13041f4de0` pins the repaired tip. That is a live observation,
not something this report can assert in advance.
