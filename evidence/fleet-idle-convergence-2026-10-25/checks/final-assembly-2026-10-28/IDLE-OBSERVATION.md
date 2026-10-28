# Task 5.2 — the pre-decision idle observation

> **Superseded:** This snapshot predates the owner's tracking-policy decision.
> `DECISION-EXECUTED.md` records that junk-runner was fixed and converged;
> the later `REMAINING-STRUCTURAL.md` identifies the remaining dirty parent as
> dracon-platform via quarantined hegemon content. The all-clean acceptance is
> still unmet, and neither document is a fresh final full-fleet observation.

## What the daemon reports now

    ok=35  active=1  warn=0  concern=0  failures=0   (of 36 watched repos)

Zero warnings, zero concerns, zero inspection failures, zero stuck push, zero
publication backlog. That is the best fleet state reached under this goal:
it began at 16 ok / 15 warn with two repositories unable to publish at all.

## The two repositories that are not "clean and idle", and why

Both are DIRTY with `push = OK`, i.e. every committed byte is published and only
working-tree content remains.

1. `junk-runner` — 127 staged + 90 modified + 22 untracked. Cause is now
   precisely known and reported by the daemon as `state_cause: dirty`:
   - The daemon-side permanent deferral is FIXED and deployed (see
     `../staged-intent-batch-limit-2026-10-28/REPORT.md`); the batch-limit
     refusal no longer appears in the journal and the epoch now builds the
     127-file commit message.
   - What remains is the repository's OWN `.githooks/pre-commit`, which refuses
     because three already-TRACKED machine-local files are staged:
     `.pi-glla/pending-approval-renders.json`, `.pi-glla/session-owner.json`,
     `.pi-glla/update-check.json`. The hook itself states the durable fix is
     "scoped root rules", and that "index-only removal is not a durable fix for
     deliberately included content".
   - Deciding what that repository should TRACK is a repository-policy decision.
     Guessing it — untracking files that are in HEAD, or bypassing the hook —
     would be exactly the "hide eligible work / bypass hooks" the objective
     forbids. Left for the owner.

2. `dracon-platform` — DIRTY only as two gitlink pointers
   (`web/games/wip/hegemon`, `web/games/wip/junk-runner`). This is PROPAGATED
   dirtiness, not an independent defect: the parent cannot be clean while a
   nested checkout is dirty. `hegemon` is DELIBERATELY QUARANTINED in the
   daemon's own `exclude_repos` ("Remove only after its divergent mirror
   histories are reconciled"), so the daemon will not commit it by design.

## Therefore the final acceptance condition is NOT met

The contract requires "every watched repository clean and idle … with zero
queued/retrying/blocked/unknown/committing/publishing states, zero inspection
failures and no outstanding convergence concerns". Two repositories are DIRTY,
so that sentence cannot honestly be written. This is recorded as a real,
narrowly-scoped remaining gap — not relabelled healthy, and not manufactured by
untracking files, bypassing a hook, or stopping the agents that edit these
repositories.

Note the objective explicitly contemplates this: "Continued editing may delay
that observation; publication progress alone does not waive it."
