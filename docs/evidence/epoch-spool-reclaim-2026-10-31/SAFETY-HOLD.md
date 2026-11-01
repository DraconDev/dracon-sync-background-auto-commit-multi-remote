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

## Corrected transaction fixture checkpoint

The old presence-only deletion body has been replaced. Staging routes private Git work through tracked producer scopes. The shared transaction requires a matching settled marker under the actual kernel fence, installs complete shared-object recovery plus exact index/metadata, writes and independently verifies compact main-missing payload, checks receipt-bound source fingerprints and journals durable intent before removal. Publication and packless sources no longer bypass these requirements.

The corrected spool suite passed 40 tests with zero failures and zero ignored, including the explicitly enabled disposable-host cases; the protected-epoch regression also passed. See [FIXTURE-QUALIFICATION.md](FIXTURE-QUALIFICATION.md), [qualified-fixtures.txt](qualified-fixtures.txt), [qualified-fixture-helper-build.txt](qualified-fixture-helper-build.txt) and [protected-release-tests.txt](protected-release-tests.txt).

These are fixture observations, not full qualification. Corrected non-vacuity, workspace gates, final review and fresh consistent-scope live accounting remain unfinished. Keep the interlock disabled; no live drain follows from these results.
