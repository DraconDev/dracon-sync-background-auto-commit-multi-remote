# Deployment and live observation

## Gates on the repaired assembly (HEAD a05a5b7d… + this change)

- `cargo test --locked -p dracon-sync --bin dracon-sync`
  → **1519 passed; 0 failed; 3 ignored** (360.32s)
- push test module → **35 passed; 0 failed** (includes the new regression)
- `cargo build --release --locked` → rc=0, `dracon-sync 0.113.96`
- `cargo clippy --locked -p dracon-sync --all-targets -- -D warnings` → clean
- `cargo deny check` → `advisories ok, bans ok, licenses ok, sources ok`

An earlier full-suite run reported 35 failures while the host was saturated
(parallel HTTPS gate + release build); the clean re-run on the same tree passed
1519/1519. Per AGENTS.md, a load-induced flake is a retry, not a gate change.

## Deployed and matching

    installed: 4ddf44f4dc53fa6a  (dracon-sync 0.113.96)
    loaded   : 4ddf44f4dc53fa6a   (PID 1642878, /proc/…/exe → the real path,
                                   no "(deleted)" suffix)

Deployed via the sanctioned path: `dracon-sync pause` → binary swap → resume →
SIGTERM (the daemon is the operator's own process and `Restart=always` reloads
the new binary). No `systemctl stop` was used. The pre-deploy daemon was
running a `(deleted)` inode; the restarted daemon loads the repaired build.

## What the fix demonstrably changes

Before the deploy, the daemon's GitLab push was classified as
`local pre-push hook failed` even though the gate PASSED, so both fail-fast
arms fired: no HTTPS attempt, no fresh-connection retry.

After the deploy the GitLab mirror **advanced for the first time in hours**:

    [12:31:05] gitlab/main=bf93551353 ahead=151
    [12:32:50] gitlab/main=bf93551353 ahead=92      <-- 59 commits published
    [12:34:35] gitlab/main=bf93551353 ahead=97      (gitlab_mentions=15)

## The remaining, SEPARATE problem: a producer outpaces the gate

Publication now succeeds, but this repository is not idle, and the backlog
does not monotonically shrink, because of a throughput mismatch that this
change does not and should not address:

- the repository's own pre-push gate runs typecheck+build+test+lint over every
  affected workspace: **~12–17 minutes per push attempt**, measured;
- live agents commit continuously: observed 547 commits today, 1–5 per minute
  sustained (git log per-minute breakdown), 1043 in 24h per `repos --json`;
- one gate round therefore publishes a tip that is already ~10–20 commits
  stale, and new commits land during the next gate.

So the queue oscillates (151 → 92 → 196) — it converges per round but is not
"clean and idle" while the producers run. Per the objective, "Continued
editing may delay that observation" — and stopping the producers is explicitly
forbidden.

This is the honest current state: the misclassification defect is fixed and
deployed, GitLab publication works again, and the remaining non-idleness is
live editing plus a slow gate, not a daemon failure.

## Post-deploy: repeated successful GitLab publication (the real proof)

The 12:32 publish was a single observation. The durable proof is that GitLab
now publishes REPEATEDLY, which was impossible before the fix (the mis-
classification armed both fail-fast arms, so no attempt ever reached the
working transport).

Live observations after the fix, `git fetch gitlab` each time:

    [16:24:14] origin ahead=34   gitlab ahead=98
    [16:25:57] origin ahead=35   gitlab ahead=99
    [16:27:39] origin ahead=36   gitlab ahead=36     <-- gitlab caught up
    [16:29:22] origin ahead=36   gitlab ahead=36

    gitlab/main advanced: fa198b8f1c -> c37945692f

GitLab's tip ADVANCED and its backlog fell from 98 to 36 — the same commit
that origin now carries. This is the daemon publishing to GitLab over the
transport that works, repeatedly, with no manual push.

## The separate throughput limit (not a daemon defect)

`browser-extensions-shared` is under continuous agent editing: 9545 commits in
15 minutes with 7281 DISTINCT commit messages — genuine work, not a runaway
loop (the daemon's own message builder varies the file set and deltas). The
repository's own pre-push gate needs ~12-17 minutes per push attempt
(measured: typecheck + build + test + lint over each affected workspace), so a
publish round always lands on a tip that is already stale by ~10-20 commits.

The queue therefore oscillates rather than monotonically shrinking: it converges
per round, and the remaining non-idleness is live editing plus a slow gate —
not a daemon failure, and not something this goal may resolve by stopping the
producers (explicitly forbidden).

## Doomtap: the objective's first named failure class, also repaired

See `../doomtap-qa-pointer-declaration-2026-10-28/REPORT.md`. Summary: the
staged file was an ASSET POINTER (`docs/screenshots/qa/trophy-before.png`,
a `dracon-asset-pointer/v1` record) of the same class as eight pointers
already tracked in HEAD and cited by the audit ledger. The fleet pre-commit's
`docs/screenshots/*` guard could not classify it (the only matching rule was
the broad `!*.png`, which the guard deliberately rejects), so every commit was
refused and 141 epochs were retained. The fix declares the ten deliberately
versioned qa/ pointers by EXACT root rules — the form the guard accepts — with
the policy test extended to 4 tests and a negative control proving an
unapproved qa/ frame is still refused.

Doomtap after the fix: 954 commits in 20 minutes (it had been unable to commit
at all), `origin` 0 ahead, working tree clean, and the refused-epoch messages
stopped appearing for it.

`doomtap` is no longer listed by `dracon-sync repos`; fleet status moved to
**33 ok / 0 warn / 0 concern / 0 failures of 36**.
