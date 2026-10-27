# Publication reassessment — 2026-10-27

The active goal is fleet convergence, not the stale bucket-strategy handoff. No acceptance claim is made.

## Fresh baseline

`fleet.json` enumerates all 36 currently watched repositories: 30 summary-ok, four active, two warned, zero inspection failures. Raw dirty/staged work, retries, stuck publication and deferred deep inspections remain. Summary-ok is not clean-idle acceptance. The retired warden-vid identity remains accounted for in earlier evidence.

## Actual configured commit hooks, without real-index mutation

Diagnostic hooks ran with private copies of the captured real index, no commit or publication. Both receipts verify the actual index still matched its captured digest afterward.

- Doomtap: original configured pre-commit exits 1. Bucket guard passes; Warden rejects four staged QA files: `docs/screenshots/qa/ascension-{after,armed,before,narrow}.png`. They are NOT any of the earlier nine approved cache de-index paths or the eight retained QA paths recorded at that time. Their source/cache role requires an operator decision; no current files/index/rules were altered.
- Platform: original configured pre-commit exits 1 because auth-api, billing-api and email-api `.env.dev` staging is not whole-file encrypted. Its private raw log remains outside Git under the diagnostic state directory because this hook can print sensitive first lines. The tracked receipt records the path and digest, not those bytes.

## Credential encryption-scope assessment — NOT applied

The operator policy explicitly removed 13 credential patterns from `protected_patterns` to stop forced ignore-negations, intending encryption to remain attached through attributes. Actual filter selection still uses protected/media scope, so attributes alone do not restore full encryption selection. The existing `media_protected_patterns` option supports text and does not add ignore-negations; a private candidate policy restored exactly those 13 patterns through that option.

The first exact-roundtrip assertion FAILED and remains recorded: `.env` cleaning increments Warden's managed comment header. Follow-up byte comparison proves all three entire bodies remain exact after decrypting, while header versions advance 1→2, 8→9, 2→3. This is NOT byte-exact preservation of the complete staged file. Live policy, staged objects and canonical files were left untouched. No encryption weakening, header-equivalence waiver, global hardening pass or manual restage was performed. The candidate policy is `/tmp/dracon-warden-scope.nKROcU/policy.toml`. A separate measurement setup failed because `/usr/bin/time` is absent; it did not execute the measured test.

## Browser original product-gate repair

An actual retained frozen hook attempt identified `ai-chat-toolkit/scripts/acceptance.ts` suppressing TypeScript with `@ts-nocheck` and declaring explicit `any`. It now uses typed CDP responses and Node's typed spawn/timer/filesystem APIs while preserving the actual Bun browser bundling invocation and all existing acceptance assertions. No real Chrome session or acceptance-browser execution was performed. Original workspace lint, typecheck, tests and production build pass. Both the driver and browser probe compile through the actual Bun CLI. Raw logs are adjacent to this report. The daemon committed the edit; no manual add/commit/push was used.

The older ai-tts-pro duplicate-key failure was already corrected in current source by other work; no second repair was manufactured. Its unused-name warning alone was not the recorded failing error. Death-note's previous failures were ENOSPC, not assertions to weaken.

## Full original browser hook remains pending / previously failed check retained

`browser-original-full-prepush.json` binds the diagnostic to immutable local revision `735065c566407cb022305219aaa2f83ccc3c5c91` and actual advertised GitHub tip `45c330977f6a9bd014c6c510c2e2e5649b8db698`. The exact original pre-push hook runs all selected workspaces, with no push and a 3600-second external deadline. The Bash tool was interrupted by a checkpoint, but a subsequent assessment observed the owned Node runner still executing; do NOT launch another copy.

Actual runner manifest: `/home/dracon/.local/state/dracon/sync/gates/browser-extensions-shared/dracon-pre-push.lwsNM1/manifest.json`; observed owner PID 2070402. Its original hook/Python parent retains the timeout. At assessment, 62 gates passed, one failed, with 31 selected workspaces total. The affiliate offline self-test exceeded Bun's unchanged five-second test deadline. A bounded native isolation of the unchanged self-test subsequently passed in 3.067 seconds. That does NOT erase the frozen full-hook failure or count as an entire original-hook pass. Assess the terminal manifest and receipt after the existing invocation settles.

No retry ledger was cleared. No security rule, meaningful assertion, timeout, publication destination or watched checkout was removed. Canonical-writer transaction qualification remains independently failed/open; no global policy migration or further deployment was performed during this reassessment.
