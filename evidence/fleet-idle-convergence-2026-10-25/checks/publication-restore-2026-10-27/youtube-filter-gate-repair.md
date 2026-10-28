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
