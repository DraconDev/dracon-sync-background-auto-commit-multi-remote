# Reclaim prototype safety hold

The epoch-spool goal is unfinished. No further live drain is authorized by the current implementation proof.

## Findings from read-only review

1. Instantaneous `git cat-file` presence, including dangling objects and alternates, is not durable preservation. An old dangling object can already be past Git's prune grace. Removing the independent spool copy can therefore lose recovery data during or after GC.
2. An index records paths, modes and stages. Retaining only object bytes does not retain those recovery semantics. The prototype removed index copies without archiving them.
3. Directory mtime is not a cross-process ownership proof. A stalled live epoch can outlive the age floor. Legacy spools have no kernel lease protocol. The in-progress ownership fix skips legacy directories and holds a kernel lock from Staging through Epoch; the settled owner still needs an owner-authorized handoff to avoid self-contention.
4. Verification must read a rescue using an isolated object database, not a repository that can satisfy reads from its existing objects or alternates. Durable installation and ledger recording must precede deletion, with crash/fault tests.
5. The workspace gate did not pass: the bounded run ended with exit 124 and contained a failed bootstrap-deadline test. Its exact failure still needs an isolated reproduction.

## Current mitigation

`LOSSLESS_RECLAIM_QUALIFIED = false` disables all calls to `reclaim_spool_if_lossless`, including the daemon sweep, CLI apply, and unpublished deadline path. This deliberately restores fail-closed behavior while preserving the prototype for correction. No daemon was stopped or paused by this work.

## Earlier live activity — not qualification

`drain.log` records earlier prototype reclaim operations and rescue claims. Those operations occurred before the above gaps were identified. They must not be represented as proven lossless. No data loss has been established, but the earlier object-presence/readback observations do not establish durable recovery or index preservation.

`baseline-before-drain.txt` and `baseline-after-drain.txt` have bounded discovery scope and are not a complete fleet accounting. The disappearance of ai-auto-writer's old largest spool remains unexplained; the speculative filter-repo diagnosis was not supported by timestamps. Do not attribute those bytes to this reaper.

## Approved contract correction

The operator-approved revised objective supersedes the original unsafe presence-only premise. It requires durable recovery closure and index/metadata preservation, cross-process exclusion through deletion, isolated verification and a durable receipt. The no-removable-duplicates criterion applies only to eligible, demonstrably inactive spools; active/fresh, marked, legacy-uncertain and failed-proof cases remain preserved and classified. The historical drain is not retroactively qualified.

## Disconnected component evidence

The independent recovery engine now uses immutable shared-object storage with separate GC-rooted recovery records and semantic receipts. Eleven focused recovery tests pass, including cancellation preserving the kernel fence, source/index reconstruction, GC survival and corruption refusal. Four producer-containment tests pass when explicitly including the disposable host fixture and using a rebuilt source-qualified helper. They exercise registered-launch cancellation, closed-stdio setsid descendants, exact environment forwarding and source mismatch rejection. See `recovery-shared-tests.txt`, `producer-qualified-tests.txt` and `producer-helper-build.txt`.

These components are not yet integrated into staging settlement or the guarded removal path. Unique-payload compact rescue/readback, final source checks, marker integration, full fault/non-vacuity/workspace gates and corrected live accounting remain unfinished. Keep the interlock disabled; no live reclamation follows from these component results.
