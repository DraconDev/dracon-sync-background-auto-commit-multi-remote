## dracon-sync v0.113.21 — richer table: submodule markers, risk markers, dim-excluded remotes

Operator audit request: "what else could we feature on the table —
we are not showing if submod or standalone repo." Four additions to
the rich `repos` table:

- **↳ nested-submodule marker** (REPO cell): distinguishes nested
  submodule checkouts (`.git` is a gitdir pointer file) from
  standalone repos (`.git` dir). Survives name truncation.
- **🩹 broken-history marker** (PUSH cell): shown when the repo has
  missing objects — the next push will fail. The config-based
  "filter-only push" case no longer exists in the daemon, so this
  makes the last invisible hegemon-class precondition explicit.
- **🔑 token-missing marker** (PUSH cell): a forge token file is
  absent for a forge this repo pushes to / is excluded from — auth
  failures visible before they become ❌ FAIL.
- **Dim policy-excluded remotes** (REM cell): active bright,
  excluded dim — e.g. `🐙🦊` + dim `🗻` under the codeberg quota
  posture, so it's obvious WHY a forge is absent.

Legend updated (REPO/PUSH/REM lines). 1213 workspace tests green;
clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
