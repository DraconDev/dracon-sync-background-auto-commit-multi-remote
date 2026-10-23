# Live observation — 2026-10-23 (installed daemon)

Read-only observations. No manual push, no state repair, no history rewrite, no
hook bypass, no coverage reduction. The daemon did all of this on its own.

## Loaded revision

* Unit: `dracon-sync-operator.service` (system, `Restart=always`). The legacy
  `dracon-sync.service` is masked on this host and was never touched.
* Installed via `scripts/install-qualified-daemon.py` (atomic replace, SIGTERM
  through a pidfd for the verified old PID). Receipt:
  `evidence/independent-publication-2026-10-23/installed-receipt.json`.
* Receipt records old PID 2026371, new PID 2034403, and the SHA-256 of the
  **newly loaded** `/proc/2034403/exe`:
  `7aaaea0314e605c5f289e514f3d5ff002e7b923071e1df7143e33bca03cb9113` — the same
  digest as `target/release/dracon-sync`. Installation metadata alone was not
  treated as proof; the loaded executable was hashed from `/proc`.

## Truthful phases, observed live

`dracon-sync repos --json` while browser-extensions-shared was mid-push:

    phase pushing, publication_phase pushing, publication_secs 99,
    execution_secs 101, current publishing, ahead 1, push PENDING

Before this change the same state reported a generic `WORKING`. `PUBLISHING`
is now a distinct state: local work is done, the tip is not on the remotes yet.

Simultaneously, `dracon-platform` reported `phase working` with `ahead 2`
`PENDING` — one repository pushing while another commits, in the same pulse.

## Commits continue behind a real slow publication

`browser-extensions-shared` journal (installed daemon):

    06:20:35 committed 2 file(s)
    06:21:54 committed 2 file(s)
    06:21:55 committed; publication owned by another worker (this pass did not push)
    06:23:46 committed 2 file(s)
    06:24:55 committed 2 file(s)
    06:24:55 committed; publication owned by another worker
    06:27:20 committed; publication owned by another worker
    06:28:32 committed 2 file(s)
    06:28:47 pre-push: 1 affected workspace(s) — running 4-gate
    06:28:47 pre-push: 4-gate passed for all 1 workspace(s)

Each `publication owned by another worker` line is a pass that committed real
content and correctly declined to start a competing upload. Previously the same
situation held the repository's whole worker and lease for the duration.

## Verified remote advancement (read-only `git ls-remote`)

    before: refs/heads/main = b1e44ed2a433ee01041fd22fde122fc54b76091f
    after : refs/heads/main = 05030a8b8904ec49286b06ba97a3148a7870ea1a

`05030a8` is a real commit object in that repository's history. The GitHub leg
advanced by two commits of work while the local branch kept growing (now 2
commits ahead again), i.e. publication made progress without blocking new
commits.

## Genuine external failure, not reported as success

    06:28:47 push to gitlab failed: git push-to-gitlab ... SIGPIPE:
             Connection to gitlab.com closed by remote host
    06:28:47 push failed (remotes: gitlab)
    06:28:49 committed but push failed (late) (will retry)

The daemon recorded a real per-remote failure (`gitlab`) while the GitHub leg
converged, kept committing, and retries on the next pulse. This is documented
as an external/remote-side failure, not as healthy convergence.

## Other rows

`doomtap` and `ai-auto-video` show dirty trees with `current unknown`. Those are
files the daemon deliberately does not commit (size/filter/exclusion gates) or
rows without a live scheduler stamp; they are not claimed healthy here. The
`aud-vis` different-worktree refusal observed during diagnosis remains an
unfixed external checkout blocker (rewiring it is explicitly out of scope).

## Baseline product gates (task 3)

Every workspace that had a retained baseline refusal was re-run through its
ORIGINAL package scripts with the hook's PATH resolution
(`node_modules/.bin` first, workspace before root):

| workspace | lint | test |
|---|---|---|
| auto-form-filler | exit 0 | 1249 passed, 4 skipped |
| youtube-filter | exit 0 | 881 passed |
| clean-web | exit 0 | 947 passed |
| SamAI | exit 0 | 953 passed |
| tabmon | exit 0 | 987 passed |
| vidpro-extension | exit 0 | 1308 passed |
| volume-and-video-pro | no `lint` script (hook skips it); typecheck exit 0, build exit 0 | 202 passed; 178 passed on the frozen initial source |

All 20 standalone workspaces were also run: every one exits 0 on its `test`
script and on `typecheck`/`compile` (three have no `lint` script, which the hook
skips). Passed-test totals range from 35 to 1308 per workspace.

Retained logs are in `product-gates/`. No assertion was relaxed, no test was
skipped or deleted, and no gate was bypassed: the earlier findings were
historical attempt snapshots, and the specific reported defects no longer
reproduce on the current code (youtube-filter's TS2554 call signatures now
typecheck; auto-form-filler's lint failure is gone; clean-web and volume pass
their full suites).

Significance limits, stated plainly: these prove the affected workspaces are
green NOW; they do not prove the original 2026-10-22 23:57 volume refusal had
the same cause, because that attempt's log directory no longer existed. That
distinction is preserved rather than papered over.

## Fleet convergence during the observation window

    first sample:  repos 36, ok 32, active 2, warn 2, failures 0
    later sample:  repos 36, ok 34, active 1, warn 1, failures 0
    latest:        repos 36, ok 34, active 1, warn 1, failures 0

The `ok` count rose from 32 to 34 with zero concerns and zero inspection
failures while the daemon was simply running. A second read-only sample
(`live-repos-later.json`) shows the two repositories that were mid-publication
earlier now `converged` with `ahead 0` and `push OK`.

`browser-extensions-shared` continued the same pattern: its tracked forge's
`main` moved `b1e44ed` → `05030a8` → `1632eed` while the local branch kept
accepting commits. Both of this repository's destinations advanced separately
over the same window — `origin` reached `1632eed9d7db5dcda51bd92790dd818cf7a15bf6`
and the `gitlab` mirror reached `d33f893d774acae5f431c94889ced431739df295`, with
local `HEAD` at `9579ac5a762175bc47805a34826d079d352b82bd`.

That is exactly the intended shape and the thing the old design could not do:
publication advances every destination repeatedly, mirror legs progress
independently of each other, and none of it blocks new commits. Local staying
ahead is not a failure — the next round publishes the newest pinned tip.
