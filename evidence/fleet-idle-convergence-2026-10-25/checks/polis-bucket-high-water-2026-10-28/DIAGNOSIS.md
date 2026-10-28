# polis: every commit refused by the bucket high-water guard

## Symptom

`polis` (nested repo) could not commit at all:

    pre-commit: bucket high-water/forward-only guard blocked this commit
    ⚠️ sync failed (late) for .../web/games/wip/polis: refused epoch message retained

The guard's own verdict:

    BUCKET_STRATEGY_HIGH_WATER
    projected pushable size 1886148605 is at or above provider limit 1885339648

Over by 808,957 bytes (0.77 MiB).

## Root cause: repeated re-encryption of one live session file

The guard's basis is the REACHABLE stored pack (`git rev-list --objects`) plus a
5% re-pack reserve plus the prospective commit's allowance:

    storedBytes          1,796,269,495
    storedPackReserve        89,813,475   (5%)
    prospective allowance         65,635
    ------------------------------------------
    pushableBytes        1,886,148,605   vs limit 1,885,339,648

The growth is one file: `pi-session-2026-10-26T11-05-09-981Z_…​.html` (56.4 MB
plaintext). `*.html` is routed through the warden `dracon` filter, which
stores it as a `[DRACON_SECRET:…]` envelope — base64 ciphertext, so ~75.2 MB on
disk per version.

That file was committed EIGHT times in eight minutes (12:24:10 → 12:31:54, one
commit per ~60s). Each commit re-encrypts with a fresh nonce, so each version is
a NEW blob:

    8 blobs × ~75.2 MB = 573 MiB stored
    = 32% of the repository's 1.79 GiB reachable pack

The plaintext barely changes between versions (56,418,171 / 56,418,092 /
56,418,137 bytes) — it is a live session export that the tooling was
appending to, so each daemon commit captured a marginally different blob and
paid the full 4/3 base64 cost again. The nonce makes the ciphertext differ
even when the payload is identical.

Total session-blob cost across all tracked `pi-session-*` files: **578.3 MiB**.

## Why the obvious remedies are wrong

- **Untracking the session files does not help.** The guard measures REACHABLE
  objects, and the blobs are already in history. Untracking removes them from
  future trees; the pack keeps them until a rewrite.
- **Raising the budget would hide the defect.** `web/config/bucket-strategy.json`
  documents that the ceiling is a GROWTH BUDGET, not a provider limit, and
  explicitly warns that re-basing "unblocked the push, it did not make the
  repository recoverable". It has been re-based once already (2026-10-07) for
  exactly this symptom. Re-basing again would repeat a known-repeated action
  while the actual growth driver is untouched.
- **The audit already established the correct rule for this repo**: the session
  AUDIT REPORT is the deliverable and is preserved in the tracked
  `.pi-glla/audit-loop/findings.md` (line 17439, which quotes the session's
  evidence inline: entry counts, tool-call census, timings, model, the
  FGA-AA-06 verdict). `docs/TODO.md` cites the same session by name for that
  report. NO test pins the raw `.html` export as tracked — the earlier audit
  that untracked `.pi-glla/audit-loop/findings.md` broke three pins, but that
  was the REPORT, which is still tracked.

## The actual defect to fix

The daemon re-commits a large, continuously-rewritten, machine-local session
export, paying a full encrypted copy per commit. Two things are wrong:

1. a 56 MB machine-local session export is tracked content at all;
2. each commit of it costs ~72 MiB of pack, so a live session's minor edits
   multiply the repository size.

The durable fix must stop the multiplication, not raise the ceiling.

## Status at time of writing

The file is no longer being rewritten (size stable at 56,403,688 bytes since
12:23:36), so the duplication stopped on its own. The pack still carries the
573 MiB, and the budget is still exceeded, so polis remains blocked.
