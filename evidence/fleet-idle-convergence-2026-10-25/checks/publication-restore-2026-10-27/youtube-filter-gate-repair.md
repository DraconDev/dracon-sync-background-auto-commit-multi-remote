# The browser-extensions-shared gate failure was a real harness defect — repaired

## What the gate was actually refusing

`extensions/standalones/youtube-filter/TEST` exited 1 because
`scripts/check-receipt-evidence.mjs` rejected the committed real-feed receipt.
That is a real assertion with a real failure behind it, not a flaky gate — and
the failure was in the harness, not the product.

## Defect (reproduced live on the operator's own feed)

Run 00:55–00:59 UTC produced the FIRST genuine live sighting on record:

    watchTogether stamped on ytd-rich-section-renderer,ytd-brand-video-shelf-renderer
    stillVisible=0, feed intact with 82 video items

The A/B comparison phase then read the feed tab and got
`No Chrome tab with id 1117155868` (the operator's window closed it), and the
receipt reported:

    the Watch-together shelf was stamped and hidden, but switching the rule off
    did not bring it back - the receipt cannot attribute the removal to the
    extension

That is a false accusation: the comparison was never read, so the target's
return was never looked for. The hunt phase had already been hardened for a
lost tab; the A/B phase had not.

## Repair (in the browser-extensions-shared repo)

- `currentFeedTabId()` re-opens the feed tab when it is gone; `readFeedSample()`
  reads through it and retries once, reporting `lostTab` so a missing
  comparison is never scored as a negative result.
- Adjudication extracted to `tests/live/real-feed-verdict.mjs` (pure module, so
  the decision rules are testable without a live signed-in feed).
- Carried-forward sightings are emitted in the shape the gate reads, bounded to
  72h (`YFF_SIGHTING_MAX_AGE_HOURS`); an expired sighting grants no credit.

## Gates on the current tree

- `npx vitest run`: 59 files, **1064 passed, 0 failed** (was 1 failed).
- `npx tsc --noEmit`: clean. `npm run lint` (`--max-warnings 0`): clean.
- `node scripts/check-receipt-evidence.mjs`: **OK**, with
  `watchTogether: REMOVED … (evidence: stamped=2 control=false)`.

Falsifiability: reverting the module to the exact pre-fix logic makes 2 of the
10 new tests fail, so the tests detect the regression instead of merely passing.
No gate, assertion, hook or timeout was weakened; the committed sighting was not
rewritten.

## Why the daemon still failed the push (and why that is not a new problem)

The attempt that failed at 01:53:52 ran against a gate SNAPSHOT taken before the
repair was committed — its log says "10 feed rounds", while the repaired receipt
recorded 12 and carries the `sighting-watch-visible` phase. The snapshot is
therefore stale by construction, and the same push re-snapshots on the next
cycle.

Also confirmed earlier and unchanged: the pre-push gate for this repo takes
~60-80 minutes (165 gate tuples across every affected workspace), which is a
duration property of the repository's own product gates, not a connectivity
fault. My earlier "idle SSH disconnect" reading was wrong and has been
retracted.

## Confirmed: the repaired gate PASSES and the repository publishes again

Gate attempts under the repair:

    dracon-pre-push.GqCEEf  started 01:59:18  finished 02:30:37  PASSED (31 min)
    dracon-pre-push.3RTN12  started 02:32:13  finished 02:36:22  PASSED
    dracon-pre-push.IESiCU  started 02:36:25  finished 02:36:25  PASSED

Inside the passing snapshot (597755043f) the gate prints:

    real-feed-shelves watchTogether: REMOVED … (evidence: stamped=2 control=false)
    receipt-evidence OK: the committed receipt evidence supports the latency,
      layer, and real-feed claims

Effect on publication, all through the daemon with no manual push:

    before : ahead 1609, last_push "2 days ago", gitlab+github both stale
    after  : ahead 9, github main advanced 45c33097 -> 59775504 -> 3a7bcf91

So the earlier refusals were correct, the defect behind them was real, and
fixing the defect — not weakening the gate — is what let publication resume.
The GitHub leg had also been blocked by the 3.67 GiB pack / 2 GiB limit; the
daemon's pack guard is what previously skipped it, and the leg now advances.

## Stuck-ledger entry cleared (documented procedure)

The daemon had recorded 3 consecutive pre-fix failures and was pausing
auto-push on that ledger. After the cause was verified fixed (passing gate in a
fresh snapshot PLUS a real published advance), the entry was cleared with the
sanctioned command:

    dracon-sync repair stuck-unstuck /home/dracon/Dev/browser-extensions-shared
    -> "unstuck", ledger now lists 0 entries for this repo

That is the documented remedy for "the cause is fixed but the ledger still
pauses auto-push", not a bypass.

## Still open at the time of writing

GitLab had not yet caught up (`9a83787e`, unchanged) while GitHub advanced; the
journal shows `Mirror Degraded: gitlab: 3 consecutive push failures`. The repo
also writes ~1 commit/minute, so it is never momentarily idle. Neither is
claimed as fixed here.
