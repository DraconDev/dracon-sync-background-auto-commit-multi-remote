# Fleet attention triage — 2026-10-18 (before v6 migration)

Operator supplied `dracon-sync repos` table with 6 repos requiring attention
and asked what to run / that these must be addressed. Parent triaged each;
no history rewrite, no guard bypass, no manual asset commits.

## Resolved by parent (daemon-committed via sync-now)

- endless-td, football-forever, polis were blocked by the forward-only bucket
  guard on staged deletions of protected legacy assets. Each deletion is
  documented intentional (endless roster cut delisting 5 enemy portraits;
  football UI overhaul deleting mascot-manager.webp; polis audit deleting the
  two noir house/tenement -v2 files). Sanctioned mechanism per
  docs/design/bucket-retirement-and-glob-fix-2026-10-07.md: exact-path
  `retiredRoots` entries added to web/docs/bucket-migration-inventory.json
  (platform working tree; pre-existing spike/ground-vector edit preserved).
  Guard --staged re-check: OK for all three. `dracon-sync sync-now` then
  committed+synced: endless 32 files, football 59 files, polis 10 files.
- .dracon from the table is now CLEAN (daemon handled it).

## Platform root: first blocker removed, second remains (code limitation)

- Was: bucket outgoing accounting `accounting-object-limit`. Cause:
  refs/remotes/github/main missing, so the duplicate-URL github destination
  was bounded by ALL reachable HEAD objects (>20,000). Sanctioned fetch under
  `dracon-sync maintenance` created github/main (f439e12d); ahead-object count
  is 810. Object-limit resolved.
- Now: sync-now refuses `accounting-unknown-filter-bound`. Cause: platform
  candidates carry `filter=dracon` (warden) attributes; outgoing accounting
  refuses to bound arbitrary filter output and fails closed. This is a daemon
  code/design limitation, not an operator command. It also blocks pushing the
  80 ahead commits because bridge prepare returns Blocked before push.
  Forward-only range guard on origin/main..HEAD is OK. No bypass applied.

## bridge-fixture

Unchanged: known user-namespace ownership refusal; v6 migration is the fix
path (administrator-handoff.md). Candidate still untracked; contract intact.

## Sequence

Operator runs only the v6 migration command (sudo, local password). No
sync-now/manual commits requested from the operator. After migration, parent
re-verifies and continues bridge-fixture qualification; platform accounting
filter-bound needs a separate daemon fix decision.
