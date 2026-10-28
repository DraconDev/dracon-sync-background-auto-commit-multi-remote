# Operator decision executed: untrack machine-local Pi state + scoped ignore rules

Operator chose option 1 (untrack + scoped root .gitignore rules) — the durable
fix the Warden hook itself names.

## What was done

`.gitignore` (junk-runner), appended after the block that had re-included
`!.pi-glla/**` for the Honesty-law artifacts:

    .pi-glla/session-owner.json
    .pi-glla/update-check.json
    .pi-glla/pending-approval-renders.json
    .pi-glla/compactor-jobs/

`.pi-glla/audit-loop/findings.baseline.md` is deliberately NOT listed: the hook's
durable-record category allows `.pi-glla/audit-loop/*.md`, and verification with
`git check-ignore --no-index -v` confirms it is still committable via the
`!.pi-glla/**` negation.

Five tracked files were untracked (`git rm --cached`), all in the hook's
"never overridden" category and none of them durable records:

    .pi-glla/session-owner.json
    .pi-glla/update-check.json
    .pi-glla/pending-approval-renders.json
    .pi-glla/compactor-jobs/mur6eolw-wlixsb/request.json
    .pi-glla/compactor-jobs/mur6eolw-wlixsb/result.json

## Worktree bytes preserved (the hook's own requirement)

`git rm --cached` refused with "staged content different from both the file and
HEAD", because the index held a THIRD, older snapshot. Rather than blind-force,
that index snapshot was preserved to disk first:

    ~/.local/state/dracon/untracked-index-snapshots-junk-runner-20261028T095444Z/
        .pi-glla_session-owner.json                (120 bytes)
        .pi-glla_update-check.json                  (46 bytes)
        .pi-glla_pending-approval-renders.json  (41604 bytes)

Then `git rm --cached -f` discarded only the index entry. Verified after:
worktree md5s unchanged for all three files, and all three still exist on disk
(119 / 46 / 42990 bytes). No worktree content was destroyed.

## Result: the three-day stall is over

Before: 127 staged files, no commit since 2026-10-25 20:50, Warden pre-commit
refusing every attempt.

After: repository's own hook returns rc=0, and the daemon committed
`49a84d143` — "129 file(s) in .pi-glla,scripts,src DELTA:+3614/-1186" — then
continued committing (~670 commits in the following 20 minutes of active agent
work, with `staged` returning to 0 each cycle). Verified stable: the three files
remain untracked in HEAD, present on disk, and ignored.

## Fleet effect

`junk-runner` and `polis` are now fully converged in the parent
(`parent-pin == nested HEAD`, 0 dirty). `dracon-platform`'s remaining DIRTY is
`hegemon` (deliberately quarantined in `exclude_repos` until its divergent
mirror histories are reconciled) plus live edits from the agents still working.
Zero warnings, zero concerns, zero failures throughout.
