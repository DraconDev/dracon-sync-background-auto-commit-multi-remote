# YouTube receipt acknowledgement repair — 2026-10-27

The operator authorized modifying their youtube-filter project. Inspection found
that the prior external-only classification was not justified: the harness had
two demonstrated defects, irrespective of whether personalized targets render.

1. Survey sampling omitted three renderer shapes used in production and counted
   ordinary `Very bad` video titles as survey controls. Fixed with 13 regressions
   and an old/new disposable control (old source reports the ordinary video,
   repaired source reports the actual survey section).
2. `setSwitches` and `restoreSettings` accepted the initial HTML title
   `YFF live probe` as completion, then closed the tab before its asynchronous
   storage readback/navigation completed. The archived pre-repair receipt
   (`before-receipt.json`, 14:00–14:14 UTC) records only that initial title for
   BOTH switch phases and restore. It therefore proves neither switch state
   nor restoration. The new shared probe acknowledgement helper requires
   explicit parsed requested/confirmed booleans, or an allowlisted restore /
   backup-forget result. No initial title, missing readback, or mismatched
   flags count as completion. Legacy settings payloads are reduced to requested
   boolean fields, never logged wholesale. Ten further regressions pass.

The receipt now avoids restoring a backup if preflight failed before any probe
mutation was attempted. SIGINT/SIGTERM request cooperative termination through
its normal restoration path, and checks between samples enforce a 14-minute
run budget. Forced-kill cleanup is not claimed or verified.

## Observed verification

- 23 targeted tests passed, zero failed (sampler + acknowledgement).
- Typecheck passed; strict lint passed.
- Full extension suite: 1046 passed, 1 failed (1047 total). The real committed
  receipt still fails its evidence check; no gate was weakened or bypassed.
- Fresh diagnostic attempt (`YFF_REAL_FEED_LOADS=2`) correctly refused BEFORE
  changing settings: loaded extension reload timestamp 11:51:09.622 UTC
  predates build timestamp 14:33:05.625 UTC. `diagnostic-run.log` preserves
  the actual exit-1 result. This was a preflight refusal, not a completed
  two-load run or a missing-target outcome.
- Chrome tool navigation to the extension page succeeded, but snapshot was
  explicitly denied: `Chrome blocks extension automation on protected URL`.
  No attempt was made to bypass that protected-page restriction.

## Next prerequisite

In Chrome, manually reload extension `cbppaanmjgeanoaelebgmhaoblhmigfd`
(AI YouTube Feed Filter) at
`chrome://extensions/?id=cbppaanmjgeanoaelebgmhaoblhmigfd`.
Then run the repaired live receipt on the current build. Fresh live evidence
and publication remain unverified; all unfinished goal scope is retained.
