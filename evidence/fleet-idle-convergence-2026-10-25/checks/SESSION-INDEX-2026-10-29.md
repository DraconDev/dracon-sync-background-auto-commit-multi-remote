# 2026-10-29 repairs — index and status

Four demonstrated causes of failed or delayed autonomous convergence were
repaired this session. Each has a durable report, behavioural regression(s) and a
disposable negative control; the two `dracon-sync` repairs are deployed and
running.

| # | demonstrated cause | repair | test | deployed |
|---|---|---|---|---|
| 1 | Automatic GC triggered only on `size-garbage`, so it could never select **reachable** loose-object bloat (the polis class, generalised) | new independent `auto_gc_loose_threshold_bytes` (64 MiB default), rate-limited probes, growth-gated repeats, unchanged plain `git gc` with prune grace | 6 auto-GC tests incl. a real repo repacked by `git gc` | **yes** |
| 2 | A local pre-push hook refusal of a **pinned** object was re-executed once per mirror leg and per retry — ~3 h in one browser-extensions-shared round over an immutable pre-fix tree | hook refusal is a decided verdict for the pinned object: mirror legs skipped, failure still recorded, round ends so the next pins the current tip | `local_hook_refusal_is_a_decided_verdict_for_a_pinned_object` | **yes** |
| 3 | `markdown-reader` LINT blocked every browser-extensions-shared push for 7 consecutive gate runs (6-7 `no-useless-escape` errors) | removed exactly the redundant backslash in 7 character classes, preserving dash-last and bracket escaping | 4 new assertions + 91,003-sample old/new equivalence check | **yes** (content committed) |
| 4 | `dracon-sync repos` timed out at 30 s with no output, so no fleet baseline could be taken | resolved by the same build; full-fleet table now completes inside its bound | live: 36 inspected, 0 inspection failures | **yes** |

## Reports

- `loose-object-autogc-2026-10-29/REPORT.md`
- `stale-pinned-publication-2026-10-29/REPORT.md`
- `browser-markdown-reader-lint-2026-10-29/REPORT.md`
- `epoch-integration-partial/raw-uncooperative-criterion-assessment-2026-10-29.md`

## Gates and deployment

- `gates-2026-10-29-decided-verdict/gates.json` — 2,917 tests / 0 failed,
  release build, strict all-target Clippy, `cargo deny` all `ok`.
- `gates-2026-10-29-decided-verdict/deployment.json` — loaded == installed ==
  gated artifact sha256 `13c7268c…`.

## Executed-artifact receipt

`/home/dracon/.local/state/dracon-sync/qualification/decided-verdict-loose-autogc-2026-10-29/installed-execution-receipt.json`
records the released binary executed against disposable fixtures:

- decided verdict: **1** hook execution vs **3** for the previous binary;
- staging: verified staged file published, unstaged edit preserved, index
  unchanged, outcome reported as `StagedPending` rather than "synced";
- loose GC: 120 loose objects (10.62 MiB) → 0 loose, 1 pack, `size-garbage` 0.

## Not established

The goal's final acceptance — a fresh full-fleet observation with **every** watched
repository clean and idle — is not met. As of 20:42 UTC: 36 inspected,
31 clean, 4 active, 1 unknown, 0 concerns, 0 inspection failures. Remaining
activity is real in-flight publication (`browser-extensions-shared` 394 unpushed
with a 44-workspace gate running; `.dracon`; `dracon-platform` retrying) plus
producer-owned dirty work (`ai-auto-writer`, `junk-runner`), not unexplained
failures.
