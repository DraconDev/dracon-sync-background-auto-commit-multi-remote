# Verification checkpoint — convergence remains unresolved

This checkpoint supersedes optimistic interpretations made during the repair session. It is NOT release approval or a completion claim.

## Validated bounded change

The push cancellation path now preserves Tokio JoinError identity through anyhow rather than flattening it into a transport-error string. Production aggregation separates interrupted-only results from real failures, including mixed cancellation/failure attempts. Interrupted results do not create or increment the stuck-push ledger; genuine failures still do.

- `typed-cancellation-focused.log`: two production-aggregation tests passed.
- A subsequent focused run added fault injection: erasing the real cancellation error's type reproduces failure recording; retaining that same error's type avoids it. Output was `/tmp/sync-fault-injection.log` (not durable until copied).
- `final-clippy.log`: affected-package all-target clippy with `-D warnings` passed.
- `final-package-tests.log`: timed out at 420 seconds; NOT a pass.
- `final-package-tests2.log`: 1054 unit tests passed, 3 ignored; 10 integration tests passed, zero failures.
- Release binary was rebuilt after `target/release` disappeared. The deletion's cause is unknown. The rebuilt binary remains version 0.113.63, not a released 0.113.64 deployment.

## Timing evidence and corrections

- Evicted-forge run: `typed-cancellation-forge-evicted.json`, dispatch 2.968s, passed its 3s gate.
- Uncached-forge run: `typed-cancellation-forge-uncached.json`, dispatch 2.742s, passed its 3s gate.
- Those runs alone do NOT prove that shutdown cancellation was exercised.
- Multiple multi-repository fairness runs failed the original timing gates. These failures remain unresolved. Eventual convergence does not satisfy the dispatch deadline contract.
- High host load was observed, but is not proof that every failure is caused by external contention. No isolated causal comparison established that claim.
- The earlier claim that repository b dispatched only 79ms after quiet expiry mixed daemon-relative and first-pulse wall-clock origins. It is RETRACTED. Python monotonic time must likewise never be subtracted directly from Unix time.
- Single-repository run `/tmp/sync-cadence-i6vr8rny`: staging at 2.935747s after edit, remote content observed at 4.407592s. Same-domain log deltas: quiet expiry to dispatch 514ms; dispatch to worker start 1ms; worker start to stage entry 225ms; stage entry to add spawn 88ms. This is a passing single-repo example, not multi-repo acceptance. The 88ms preparation interval is not itself a defect.

## Remaining release blockers

1. Establish and satisfy failing-remote and multi-repository fairness acceptance with trustworthy timing attribution, without relaxing bounds or discarding failed runs.
2. Finish validation of diagnostic parsing changes; they are not authority to reinterpret failed acceptance checks as passes.
3. Review full worker cancellation/descendant teardown ownership before claiming exclusive ownership for all failure paths.
4. Release/install only after gates are satisfied, then verify running version and remote release refs.
5. Repair the live observer, complete a valid 15-minute installed-pipeline observation, and reconcile the investigation report with raw evidence and visible exclusions/blockers.

Do not fabricate a production bug from a passing 88ms preparation interval, weaken encryption/size controls, stop the live sync service, or treat a lower-load pass as proof that observed high-load failures do not matter.
