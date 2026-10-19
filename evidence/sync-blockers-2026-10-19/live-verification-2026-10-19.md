# Live verification — four-repository blockers (2026-10-19)

Deployed daemon: 2fc714a1252ed64f3102175a67587c848567629df9b8e099d38d45789744a2cd
(service dracon-sync-operator.service, pid 2039261, system manager, UID 1000;
rollback binary retained at
/home/dracon/.local/state/dracon-sync/deployments/20261019T111245Z-blocker-fix/dracon-sync.before).
Installation used sanctioned `dracon-sync maintenance` (freeze) — the service
was never stopped.

## Post-fix live outcomes (local 11:32–12:31)

| Repository | Before | After |
|---|---|---|
| dracon-platform | BLOCKED, accounting-unknown-filter-bound refusal, 129 ahead, no commits | committed 100 files 12:24:07; pushed 12:26:28; origin/main == gitlab/main == 6365098270d58 |
| doomtap | BLOCKED, forward-only guard refused the two staged vocal deletions | retirements recorded; the deletion commit bb27dfdb178c (11:32:23) plus later commit 0ddc365b8b77 (11:56:43) pushed to origin+gitlab; state clean |
| polis | BLOCKED, forward-only guard refused the seven staged image deletions | retirements recorded; commits pushed (12:26:18 commit 97fa4c1fb130); state clean |
| browser-extensions-shared | push FAIL 11:01 (cause not captured) | 12:23:38 commit 141364a6f502 pushed to origin+gitlab; push recovered 11:17:48; push stdout is now captured (bounded, redacted) so a future refusal explains itself |

Unrelated repositories continued to commit and sync throughout
(.dracon, db-gateway, junk-runner, endless-td, capture-anime-girls).

## Guard integrity

- No guard bypass, no history rewrite, no force-push, no credential change.
- Warden, forward-only asset guard, GitHub pack guard and high-water sizing
  all still active; the nine retirements are exact-path inventory entries.
- Platform root bucket opt-in disabled ONLY after explicit operator choice
  (monorepo is outside the standalone plain-Vite consumer adapter).
- No temporary injection remains; no maintenance freeze remains.

## Known limitation (unchanged, out of scope)

The operator-reported 2026-10-19 00:19:10 `reset: moving to HEAD` incident
is recorded in web/jobs/note.md with its restoration commits. Reflog does not
identify the caller or the --hard flag, and current sync production resets
are index-only. That incident explains lost app edits; it does not explain the
independently reproduced accounting/retirement blockers fixed here.
