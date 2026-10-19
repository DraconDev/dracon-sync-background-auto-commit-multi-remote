# Four-repository sync investigation — 2026-10-19

## Established root causes

- dracon-platform: outgoing.rs rejects every filter-bearing worktree path,
  including ordinary Warden filtering, before the upload runner or commit.
  This also prevents the push phase for 126 ahead commits. A raw length or
  filter-name exemption would undercount encryption expansion. Correction:
  reserve the finite per-path staged-output ceiling already enforced by
  normal staging, without executing filters for estimation; backstop that
  ceiling with actual staged-blob measurement in the final bucket gate.
- doomtap: intentional removal of vocal-crit-grunt.mp3 and vocal-death.mp3
  under static/assets/audio is absent from retiredRoots. Intent is explicit
  in src/lib/audio.ts:119-133 (operator reversal), scripts/lib/asset-inventory.mjs
  removes generation recipes, and audio tests require only boss/victory vocals.
- polis: intentional removal of dock/farm/granary/guardhouse/library/market/well
  -v2.png under static/images/polis absent from retiredRoots. Exact paths are
  documented individually in docs/AUDIT-2026-10-18-non-housing-wrong-assets.md;
  src/lib/visual-assets.ts and visual-assets-manifest.json route to retained
  Greek originals. All seven originals exist. Nine exact retirements added
  to platform inventory, no broader roots changed; diagnostic staged guard
  passes for both games (no-size diagnostic is NOT push approval). The two
  pre-existing polis entries (screens/olympus-bg-v3.png, bandit.png) were
  retired on 2026-10-12 (commit 4656f4c32b) and 2026-10-13 (cbd9a28450)
  and pre-date this goal; only the seven -v2.png and two vocal .mp3 paths
  are new here.
- browser-extensions-shared: failure had self-recovered by 11:17:48 local;
  daemon logs subsequent autonomous commits and pushes at 11:22, 11:24,
  11:29. A bounded dry-run with hooks enabled passed tabmon's four gates.
  Original daemon stderr says only 'failed to push some refs'; its stdout
  is discarded, so historical hook-vs-network cause is not yet established.
  Do NOT pretend the current successful dry-run proves the original cause.
  Durable repair: push stdout is now captured, bounded and credential-
  redacted, so a future gate refusal is self-explaining (see
  browser-extensions-push-cause.md).

## Misleading hints

Current classification was applied after generic ahead/dirty hints, leaving
BLOCKED rows promising 'after changes settle' or recommending force sync.
Correction selects a truthful guard/policy/journal hint only for affirmative
Blocked evidence, preserves concrete existing diagnoses and pack suffixes,
and leaves active Working rows unchanged.

## Reset incident reported by operator

Platform reflog confirms reset: moving to HEAD at 2026-10-19 00:19:10.
web/jobs/note.md records observed loss of tracked ingest/Button-export edits
and restoration commits 8172c6a/b1853fb/622933c/b8daeb9. Reflog alone does
NOT identify --hard or the caller. Current sync production reset calls are
index-only; --hard matches inspected in git/mod.rs and sync.rs are inside
test fixtures. This incident may explain lost app edits and app gate failures,
but does not explain the independently reproduced accounting and intentional
retirement blockers. No current worktree was reset or restored in this audit.
