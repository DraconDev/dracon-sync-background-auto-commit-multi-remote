# Remaining blocker for task 5 (operator action required)

Exactly one watched repository prevents the required "every repository clean and
idle" observation: `browser-extensions-shared` (AHEAD:1585, push PENDING).
The other 35 are clean or DIRTY-with-OK-push (ongoing editing, nothing stranded).

Its push is correctly REFUSED, not failing. The daemon runs the repository's
own pre-push gate to completion (~78 min, inside the scaled 90-min window) and
then the gate exits 1 on one real assertion:

    extensions/standalones/youtube-filter/TEST exited 1

Cause: `extensions/standalones/youtube-filter/tests/live/evidence/real-feed-shelves-receipt.json`
records that the live YouTube feed did not render the targets, so the run cannot
demonstrate removal. `scripts/check-receipt-evidence.mjs` rejects it (reproduced
directly: 1 failed | 24 passed in check-receipt-evidence.test.ts).

## Why I cannot clear it

The documented next prerequisite is a manual browser action:

> In Chrome, manually reload extension `cbppaanmjgeanoaelebgmhaoblhmigfd`
> at `chrome://extensions/?id=cbppaanmjgeanoaelebgmhaoblhmigfd`.
> Then run the repaired live receipt on the current build.

Attempted and confirmed blocked by the platform, not by policy:

- `chrome_snapshot` / `chrome_screenshot` on that tab →
  `Chrome blocks extension automation on protected URL`.
- The Pi Chrome Connector lacks the `management` permission, so
  `chrome.management.reload(id)` is not available from a regular page.

Reloading the extension is a normal development action, not a gate bypass, so
it is permitted — it is simply not automatable from here. The subsequent live
receipt also needs a real signed-in YouTube home feed to render the target
shelves, which is a live-feed condition I cannot manufacture.

## Second, independent blocker on the same repository

Even once the gate passes, GitHub cannot accept this repo: pack is 3.67 GiB
against GitHub's 2 GiB limit. GitLab is viable. Clearing GitHub needs asset
migration out of Git history (the bucket-strategy work), which is a separate
operator-scoped project — not something to attempt under this goal.

## What is NOT blocked

Publication is restored and working: 17 repositories converged autonomously and
32/33 clean repos were verified at every intended remote. Re-lifting containment
is not needed; nothing was left contained.
