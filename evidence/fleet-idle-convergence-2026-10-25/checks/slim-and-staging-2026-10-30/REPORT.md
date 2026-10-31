# Two structural blockers removed (2026-10-30, operator-authorized option 1)

The fleet could not converge because two repositories were structurally unable
to commit — not slow, unable. Both are now fixed and both were verified by the
thing that previously failed.

## (a) `polis` — slimmed under the sanctioned procedure

`polis` was blocked by its **own** nested bucket budget, not the platform's:
`pre-commit` failed closed with `BUCKET_STRATEGY_HIGH_WATER`,
`projectedBytes=2345216557 >= highWaterBytes=2344615936` (over by 600,621 B).
Object store 2.77 GB, reachable pack 2,184,647,774 B.

Bloat was the `pi-session-*.html` class — **25 unique blobs, 1,656.5 MB** of pi
session dumps, an AGENTS.md-excluded class committed before
`.gitignore:259` existed. They were already history-only (0 tracked in `HEAD`),
so the rewrite could not change any shipped content.

Executed the full `docs/design/sanctioned-slimming-2026-09-26.md` procedure:

| step | result |
|---|---|
| bundle backup | `~/dracon/backups/polis-pre-slim-20261030.bundle`, 2,183,238,398 B, `git bundle verify` = "records a complete history", `refs/heads/main` IS the cutover old tip `4127c384b1` |
| scratch rewrite | `filter-repo --invert-paths` on the 3 paths, outside the watch roots |
| guard-verify | PASS: `projectedBytes 918,951,185` vs `2,344,615,936`, `remaining 1,425,664,751`, `forwardOnly.ok: true` |
| cutover | compare-and-swap `update-ref`, then `--force-with-lease=<old>` + `--no-verify` to github, gitlab, origin in one `maintenance --` window with `DRACON_ALLOW_REWRITE=1`; GitLab `main` unprotected for the window and re-protected after |
| gitignore | already satisfied by `.gitignore:259`, verified with `check-ignore --no-index` |
| re-verify | live guard `ok: true`, `projectedBytes 918,504,169`, `remaining 1,426,111,767` |

**Result: 2,184,647,774 B → 876,021,960 B (59.9% smaller).** All three remotes
now at `0570a34b0d802bb80a71fa6d1712e84ec78779ef`.

Preservation is proven, not asserted: the rewritten tip's **tree SHA is
identical** to the pre-slim tip (`cbab7de036ecdc7afbd2e6f5853d2d370247f75c`
both sides), every one of the **6,939** non-bloat object paths present before
is present after (`comm -23` → 0 lines), and the commit count moved only
16126 → 16106.

Effect: `polis` went from *permanently unable to commit* to committing 41 files
and publishing (ahead 0, dirty 0).

## (b) `dracon-strategy` — a real staging-budget defect

This one was **not** a policy question, contrary to my earlier assessment. I had
reported that a 158.3 MB file exceeded `max_stage_file_bytes`; that was wrong —
`market-research/rising-stars/raw/channel-raw.jsonl` is **already gitignored**,
and `git ls-files --others --exclude-standard` shows the staging surface is only
533 files / 84.2 MB. So the size limit was never the blocker.

The real cause, from debug instrumentation:

```
worker_prepare ... after_diff   elapsed_ms=66078..129047   <- Warden clean filter
                                                              over the whole diff
stage_prepare  ... partition    elapsed_ms=62
actual staging                  ~86-96s  (bare `git add -A --dry-run` measured)
```

`stage_op_timeout_secs` (code default **60**) is ONE absolute deadline shared by
the entire preparation, and it starts at epoch capture — so the Warden diff
phase alone consumed it before `git add` began. Every cycle aborted with
`ordinary staging absolute deadline expired; candidate retained`, so the repo
could never commit.

Fixed by sizing the operator override from measurement rather than a round
number: `stage_op_timeout_secs = 300` in
`~/.dracon/utilities/sync/dracon-sync.toml`, with the measured phase breakdown
recorded inline. The bound stays finite and enforced — this raises a limit, it
does not remove one — and the Warden filter (the correct security path) is
untouched.

Effect: `dracon-strategy` committed **611 files**, then reached
**dirty 0, ahead 0, untracked 0** with everything published.

## Fleet effect

`concern=0, failures=0, pending=0, warn=0` in the cleanest sample after these
repairs; the remaining non-converged rows are repos with gates actively running
and producers writing every few seconds (`browser-extensions-shared` had
`run-push-gates.mjs` live and a commit 3 seconds old). No producer was stopped,
no hook bypassed, no history rewritten outside the authorized polis slimming,
and no manual recovery push was used.

## (c) Retained private-epoch spools: a bounded, deliberate tradeoff — NOT deleted

While verifying `ai-auto-writer` I found **41 top-level
`.git/dracon-staging-*` directories holding 5.04 GB**, 9 of them created in the
last hour, with one `retained-*.pack` alone at 4.75 GB. `reap_abandoned_spools`
is a deliberate no-op:

```rust
// Neither age nor a process-local registry proves that retained objects
// are published or unused by a different daemon/CLI process. Fail closed.
0
```

I measured redundancy directly rather than trusting the age heuristic: of the
**87,402 blobs** in that pack, **87,400 are already present in the main object
store**. So it is 99.998% redundant — and 2 blobs exist *only* in the spool.

That is exactly why the reaper fails closed, and why I did **not** delete it.
Removing it on an age or redundancy heuristic would be the "age-only deletion is
unsafe" failure the code warns about. Recorded here as a real disk cost with a
real reason, not as something fixed. The repo itself converged normally
(`dirty 0`, `ahead 0`) once its commit went through, so the spools do not block
convergence — they cost space.

## (d) Final state — converged except under continuous production

After both repairs, twelve samples spanning ~35 minutes:

| sample | converged | warn | concern | failures |
|---|---|---|---|---|
| s1 | 29/36 | 3 | 0 | 0 |
| s4 | 28/36 | 4 | 0 | 0 |
| s6 | 31/36 | 0 | 0 | 0 |
| s10 | 31/36 | 2 | 0 | 0 |
| s12 | 30/36 | 3 | 0 | 0 |

`concern=0` and `failures=0` in every sample, and the report's own `ok`
now equals the independent per-row derivation in all twelve.

The remaining rows are not stuck. Direct observation:
`browser-extensions-shared` fell from **75 → 15** unpushed commits (publication
draining), its `run-push-gates.mjs` and the per-workspace
youtube-filter LINT/TEST logs were actively being written, and producers commit
every few seconds (`25b877a94d` was 3 seconds old when sampled). The gate cycle
takes ~30 minutes per push, which is slower than production in that repo, so
that row oscillates rather than settling. `open push` holding at 15 while HEAD
keeps moving is the expected shape of that.

So: the fleet is converged except where it is actively producing, and no
repository is structurally unable to converge any more. That is materially
different from the starting state, where `polis` and `dracon-strategy` could
never commit at all and `browser-extensions-shared` was six hours behind.

Gates on the final assembly: 44 suites, **2923 passed / 0 failed**, release
build exit 0, strict Clippy exit 0, `cargo deny` all-ok.
