# Scheduler UNKNOWN and browser gate retries — 2026-11-01

## Demonstrated problems and repairs

1. `scheduler_evidence::build` originally enumerated only activity, probe,
   maintenance, retry and recovery paths. A repository appearing only in the
   explicit eligible/admitted inputs disappeared entirely. The snapshot now
   includes both inputs independently. Admission alone does **not** become
   execution or current eligibility; unavailable evidence still stays unknown.
2. UNKNOWN could retain a generic `healthy` hint or speculative `repair
   concerns --apply (push or rewrite)` advice. Those generic hints now explain
   missing scheduler evidence. Concrete refusal diagnostics and pack/history
   qualifications remain visible. This is not a license to call unknown clean.
3. Browser push attempts repeated completed work after a bounded attempt
   failed or was cancelled. The runner now records checkpoints for complete,
   successful **leaf-workspace** gate sets. Reuse requires the exact source
   commit, runner/lifecycle/verifier, Node executable/version, npm version,
   environment digest and gate list. Original success logs must exist with
   matching hashes, safe paths and ownership, and be less than 24 hours old.
   Partial, failed, altered, missing or chained evidence is refused. Preparation
   and dependency workspace builds always execute. Affiliate checks are not
   cached. No deadline, lint rule or security hook was disabled.
4. Retained original-hook attempts I4WAEY and DnSLvk exposed three real lint
   failures beyond timeout churn: markdown-reader's unnecessary template-string
   escape; unkeyed numeric SVG loops in web-time-tracker-limiter; and minified
   modern-4chan preview output being linted as source. Fixed the escape and
   keyed loops. Only the generated `modern-4chan/preview-dist` output is excluded
   like dist/. Its authored sources still fail on meaningful lint regressions.

## Verification so far

- Rust focused tests: 3 passed. Reverting the scheduler enumeration and hint
  conditions made both new regressions fail; restored fixes pass.
- Hook runner/lifecycle/receipt suites: 46 passed, zero failed.
- Retry integration negative control: disabling reuse made the production-runner
  fixture fail. The positive fixture still rejects a failing second workspace
  while reusing only the first workspace's complete success; new source forces
  execution again.
- Generated-output lint boundary: passes, including the former-config negative
  control, authored-code failure control and harmless-source success control.
- All four product gates pass for markdown-reader (154 tests), modern-4chan
  (217 tests), and web-time-tracker-limiter (470 tests). Lint warnings remain
  warnings; no rules were relaxed.

## Incomplete qualification / real limitations

The full Rust workspace run FAILED: four spool-reclamation assertions fail
reproducibly, while one child-wakeup timing test failed only under full-suite
load and passed the focused rerun. Strict Clippy also FAILED in the concurrently
changing spool recovery/producer code (byte-char-slices, needless-return and
nested format arguments). Release build and cargo deny passed. These failures
are not represented as passing gates.

The spool redesign explicitly keeps `LOSSLESS_RECLAIM_QUALIFIED = false`;
its design is documented in epoch-spool-durable-recovery-2026-10-31.md.
Do not remove that safety hold or weaken its assertions to ship these reporting
changes. This session has NOT redeployed the daemon with the reporting changes
while workspace qualification remains red. Hook JavaScript is read by subsequent
normal daemon-owned push attempts; that is separate from daemon deployment.

Browser retry budget was re-enabled only after the three demonstrated product
failures passed all four gates locally. Fresh original-hook attempt PUiPPG
started at 2026-11-01T03:17:41Z against d11d686b685790db0aedcd690abe37ad6319f020.
At capture it was running with no failed gates yet; this is NOT a publication
success claim. No manual recovery push, history rewrite, producer stop, or
security-policy relaxation was performed during this repair.
