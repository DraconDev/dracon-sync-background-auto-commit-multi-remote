# Self-healing pipeline — task list (2026-11-01)

Goal of this list: publication keeps working **without a human digging into
`~/.local/state/dracon/sync/gates/**`**. Every incident this session had the same
shape: the pipeline knew something failed but never said *what*, and never
recovered on its own.

Ordered by leverage. Each task states the evidence that motivates it and the
acceptance that closes it.

---

## Already done (context, not open work)

| fix | evidence |
|---|---|
| Retry budget is **per-revision** (`failed_tip`) | browser stalled forever on `2cf8c70e`, 36 commits behind HEAD; now auto-resumes |
| Gate **checkpoints** reuse passed leaf workspaces | one attempt reused 23 gates instead of redoing 44 workspaces |
| Stale **ref/HEAD lock** janitor | `refs/heads/main.lock` wedged dracon-platform 6.3 h |
| **tmp_pack** reaper with `fuser` proof | 12.60 GiB reclaimed; daemon reaped 2 packs live |
| Report truth (`ok` was a residual) | 14 dirty rows previously reported as 36 clean |
| CLI layout: no wrapped cells | `📤 PUBLISHING` wrapped → looked like an extra row |

---

## T1 — Surface the gate-failure cause (P0, no dependency)

**Evidence.** Today the operator sees
`Stuck Ahead (Unpushed): commits not reaching origin for >10 min — push may be failing`
and the hint says `run dracon-sync repair concerns --apply (push or rewrite)`,
while the real cause was `bun install` → `Workspace not found` at `2cf8c70e`,
then `virtual-pet-new-tab TEST failed`. The cause exists only inside a retained
attempt directory.

- [x] **T1.1** Stuck-ledger `last_error` carries the failing **workspace, gate
      and log directory** via `hook_failure_summary()` (pure, bounded to 200
      chars, front-loaded). Done — deployed `679347be1104`.
- [x] **T1.2** The alert/HINT inherit it: `push_stuck_hint` reads the ledger
      error, and its budget was raised 60 → 160 chars so the cause survives.
      Done.
- [x] **T1.3** Durable artifact at `<state>/dracon/gate-failures/<repo>.json`
      (atomic temp+rename, no credentials, `detail` names the gate + logs).
      Done.
- [x] **T1.4** HINT no longer says "push or rewrite" for a gate refusal once the
      budget is exhausted — it names the gate. Done.
- [x] **T1.5** Two regressions + negative controls (neutralising both made 3
      assertions fail); four gates 3018 passed / 0 failed; deployed with
      loaded == installed == built. Done.
- [x] **T1.6** *Residual, CLOSED 2026-11-02:* artifact populated live at
      02:09:30Z (`gate-failures/browser-extensions-shared.json`) — but its
      `detail` was only `logs dracon-pre-push.0WWc5A`. Cause below (T1.7).
- [x] **T1.7** (found + fixed 2026-11-02) The gate marker is MID-LINE live:
      `ops.rs::child_status_result` composes `"{label} failed …: {stderr}"`,
      so `  - dir/GATE exited N` lands after the prefix on the composed
      first line; the T1 extractor required line-start `- ` and missed it.
      Fix matches `- <label> exited <code>` anywhere (label must contain
      `/`, so install excerpts stay with the reason arm). Regression uses
      the live 02:09:30Z error text; negative control pins the install
      case. Product cause that night: virtual-pet-new-tab TEST refused by
      `check-privacy-date.sh` (privacy.md stamped 2026-09-13, committed
      2026-11-01) — re-stamped via the script's own `--fix`, committed
      `c787888ddb` in browser-extensions-shared. Pending at write time:
      deploy, `stuck-unstuck`, confirm the gate passes on the fixed tip.
- [x] **T1.8** (found + fixed 2026-11-02) T1 was wired only into the
      COMMIT path (`stage_commit_and_push`); backlog pushes go through
      `handle_ahead_push`, whose identical arm mined nothing — two live
      gate refusals recorded generic while the extractor was proven
      correct. Fix extracts a shared `push_failure_message` composer used
      by BOTH arms (+ artifact write on both). Same change also mines
      warden secret refusals (`Possible plaintext secrets…` + the hook's
      `Run: dracon-warden once …` line), which legitimately mined to None
      before (empire, live). Tests: composer unit test + warden-shape
      regression (live empire text). Pending at write time: gates,
      deploy, live confirmation on the next refusal.

**Acceptance.** After a deliberately broken test in a fixture workspace, `repos`
and the alert both name the workspace, the gate and the log path — verified with
no manual log reading.

## T2 — Make the largest bucket manifest declarable (P1)

**Evidence.** `books.json` declares 952 assets / **571.9 MB** — the largest
offload — but cannot be wired into `bucket-migration-inventory.json`:
`bucket-strategy check` rejects it because its `objectKey` uses **literal
digests** (952/952) while the schema requires `…/{sha256}.{ext}`. The runtime
ignores the field (`namespace/sha256.ext`), which is why it serves correctly
while being unvalidatable. `bucket-sizes` therefore prints
`books | not declared` and cannot see 572 MB of 578 MB.

- [x] **T2.1** Rewrote all 952 `objectKey` values to `books/{sha256}.{ext}`.
      Proven representational first: all 952 literal keys already equalled
      `namespace/sha256.ext` (952/952, 0 divergent), so only the *form*
      changed. Also normalised the 11+11 private-manifest entries.
- [x] **T2.2** Wired into the `books` inventory record. `bucket-sizes` now
      reports `books | 571.9 MiB; 952 keys; 0 unknown` (was `not declared`).
      3/3 sampled covers still serve byte-exact after the rewrite.
- [ ] **T2.3** Operator decision: leave `classification: legacy`, or promote to
      `declared` (a migration-state claim, not a mechanical change).
      2026-11-02: T2 verified LANDED in platform HEAD (all three manifests
      0 non-canonical keys), `bucket-sizes` prints
      `books | 571.9 MiB; 952 keys; 0 unknown`, `check` shows only the 2
      pre-existing errors (outpost-eden, web/jobs). Landing required a
      manual explicit-path `git add` of the two private manifests: the
      daemon would not stage `games-private.json` for 75 min, so every
      epoch round died at `verify_staged` ("staged private manifest
      differs…"). Follow-up: daemon-side staging/verify mismatch for
      private-manifest updates (no code changed — needs its own task).

**Acceptance.** `bucket-sizes` reports ~572 MB for books; `check` stays green.

## T3 — Remove the decorative `objectKey` divergence (P2)

**Evidence.** Three key shapes coexist: canonical template (`games.json`),
slug-prefixed template (`games-private.json` → `games/covers/<slug>/{sha256}`,
which the runtime ignores), and literal digests (`books.json`). `bucket-sizes`
reads `asset.objectKey`; `bucket-asset-transfer` derives its own. That
divergence made my first verification report **1/12** assets present.

- [x] **T3.1** The real bug was narrower than "divergence": `validateManifest`
      compared the **template string**, so collapsing 952 entries onto one
      canonical template produced 951 "duplicate" errors. It now compares
      **resolved** keys (`resolvedKey()`), which is what made T2 possible.
- [x] **T3.2** Attempted and **rejected** a "no extra path segments" rule: the
      public catalog legitimately uses `games/<slug>/<path>` layouts (a test
      pins `games/example/brand/`), so that rule broke 5 real tests. Recorded
      rather than forced; the resolved-duplicate check is the durable fix.

**Acceptance.** A manifest with a wrong `objectKey` fails validation instead of
silently computing a key that does not exist.

## T4 — Bucket coverage for the oversized repos (P1, needs classification)

**Evidence.** `ai-auto-writer` is **7.25 GiB** with no bucket manifest and is not
in the inventory (which covers only `dracon-platform`, 32 records). On-disk
history by class: `.jpg` 2.24 GiB (31%), `.png` 2.20 GiB (30.5%), `.epub`
1.29 GiB (17.9%), `.md` 1.08 GiB. `.log` is 4.59 GiB raw but **0.01 GiB** on
disk — a red herring that cost me a wasted slimming run.

- [ ] **T4.1** Operator classification: are `.epub` (built from `.md`) and
      `cover.png`/`.jpg` regenerable artifacts or shipped product?
      2026-11-02 analysis: 1,555 tracked `.epub` ARE build artifacts —
      `BookFormatter::to_epub_with_cover` (`src/output/book_formatter.rs`,
      via `src/cli_handlers/convert.rs`) regenerates them from the book
      sources. 3,093 jpg/png covers come from AI batch scripts
      (`scripts/batch_covers_codex25.py`, …) with repair loops — plausible
      curation, needs the product call. Coherent split: bucket+slim the
      epubs now, decide covers separately.
- [ ] **T4.2** If regenerable: manifest + move to the bucket, gitignore.
- [ ] **T4.3** Sanctioned slimming with a verified bundle; tip tree must stay
      byte-identical where the class is history-only.
- [ ] **T4.4** Verify: guard passes, all remotes agree, reachable pack measured
      before/after.

**Acceptance.** Reachable pack below the repo's budget with no shipped content
lost, or a recorded decision that the size is accepted.

## T5 — Restore `freeport`'s git identity (P0, operator-only)

**Evidence.** `user.name`/`user.email` are literally `--global` (a misordered
`git config user.name --global …`). 60 commits authored `--global <--global>`
since 15:04, already published; daemon warns `path_owned:untrusted_email`.
No other repo is affected.

- [ ] **T5.1** Operator runs the two `git config` commands (agents may not).
- [ ] **T5.2** Verify the next commit's author is `DraconDev <dracsharp@gmail.com>`.
- [ ] **T5.3** Optional: mailmap the 60 published commits so history displays
      correctly (no rewrite).

## T6 — Spool reclamation (blocked, do not start)

**Evidence.** 213 dirs / 0.77 GiB fleet-wide and slowly growing.
`LOSSLESS_RECLAIM_QUALIFIED = false`; `src/git/epoch_spool_*.rs` is under active
concurrent redesign with its own revision-pinned test helper.

- [ ] **T6.1** Wait for that redesign to land and qualify.
      (2026-11-02: still blocked — `LOSSLESS_RECLAIM_QUALIFIED = false`;
      last spool-file commits Nov 1 12:49–12:53Z.)
- [ ] **T6.2** Then reclaim with content proof (not age), preserving
      spool-only objects as was done for ai-auto-writer's 9 objects.

## T7 — Investigate a new signal (P2)

**Evidence.** Alert observed:
`capture-anime-girls — Sync Failures (backing off): 5 consecutive sync failures;
probing every 15 min`. Not yet diagnosed.

- [x] **T7.1** RESOLVED 2026-11-02 (no shared root cause with T1). The phase
      is the forward-only guard: it flagged 12 staged card renames
      (`char_250–261.png` → `char_5300–5311.png`) as `rename-away`. All 12
      are R100 same-blob (zero new bytes), but the rule fires on the OLD
      path alone by design, and the test suite pins retirement as the
      escape hatch. Fix used the sanctioned mechanism, no logic touched:
      added the 12 exact old paths to the `games/capture-anime-girls`
      record's `retiredRoots` (freeport-fx precedent). Guard re-run:
      `ok: true`, zero violations; inventory `check` still shows only the
      2 pre-existing errors. Side note: more new (non-rename) `char_53xx`
      cards are untracked — additions don't trip this guard, but they
      grow the 1.93 GiB repo; bucket coverage for games is still open.

---

## Producer-owned product failures (2026-11-02, recorded for the game loops)

Genuine TEST failures blocking browser-extensions-shared publication.
Behavioral (intended-behavior calls), producer active in both areas —
recorded, not fixed, per the T1 design (the loop sees the named gate
and fixes it):

- **tabmon** (`extensions/standalones/tabmon/tests/user-flows.test.ts:283`,
  failed at tips 635c3830 + later): sound toggle flips the label to
  "Off" but `tabmon_settings.soundMuted` is not `true` in storage.sync
  — UI flips, persistence doesn't (or writes the wrong key/version).
  1 failed / 1293 passed.
- **clean-web** (failed at tip a2d46c46): 5 engine TEST failures —
  forum-post node grouping, payload-block segments, byline/price/date
  reach, desktop-twin byline, OPEN burst budget (`fails OPEN when the
  burst carries more text than a scan can afford`, 68s — also slow).

## Empire secret-scan wedge (2026-11-02, fixed via sanctioned hatch)

`Possible plaintext secrets` refused empire's push (79 unpushed): the
scan matched `password=hunter2` fixtures in `src/probe_tests.rs`
(redaction regression suite — the test asserts those fakes get
scrubbed) and `password=REDACTED` in `src/demo.rs` (demo scrubbed-
output marker). False positives. Fixed with the established
`.plaintext` sibling hatch (+ justification docs, self-verified
scan-clean), platform precedent
(`refresh_policy_test.rs.plaintext`). Hatches committed `8725c1a`; push recovered same hour (79 commits,
0 ahead, ledger clear) — hatch confirmed working live. Same session:
my own audit prose in this doc quoted those fixture shapes and wedged
dracon-sync's own push the same way; hatched this file too
(audit-doc precedent, `findings.md.plaintext`).

## Standing rules for this list

- Every fix ships with a regression **and** a negative control that fails when
  reverted; no gate is narrowed or skipped.
- Deployment requires loaded == installed == built and a green four-gate run; a
  load-induced timing flake is retried, never excused by weakening a test.
- Agents do not run `git config user.*`, rewrite history, force-push, or stop
  producers (T5 is operator-only).
- Any claim about bucket contents must be verified by fetching the object, not
  by reading a manifest.
