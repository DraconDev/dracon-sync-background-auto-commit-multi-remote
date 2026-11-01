# Corrected reclamation fixture checkpoint

This is the fixture milestone, **not approval for a live drain**. `LOSSLESS_RECLAIM_QUALIFIED` remains false. The earlier prototype's drain and mutation record do not qualify this redesign.

## Reproduction and observations

From `dracon-sync/`:

```sh
cargo build --locked -p dracon-sync --bin dracon-sync
DRACON_EPOCH_TEST_HELPER="$(realpath ../target/debug/dracon-sync)" \
  cargo test --locked -p dracon-sync epoch_spool -- --include-ignored
cargo test --locked -p dracon-sync --bin dracon-sync \
  settled_epoch_preserves_original_conflicts_and_rejected_bytes
```

The recorded runs used caller-side timeouts of 240 seconds for the build and spool suite, and 60 seconds for the protected-epoch regression. All exited 0; none reached its timeout.

- **40 passed, zero failed, zero ignored** in the spool suite, including both explicitly enabled disposable-host cases: [qualified-fixtures.txt](qualified-fixtures.txt).
- The source-matching helper was built successfully without compiler warnings: [qualified-fixture-helper-build.txt](qualified-fixture-helper-build.txt). The host trampoline checks its source identity before executing its target.
- **One protected-epoch regression passed**, zero failed: [protected-release-tests.txt](protected-release-tests.txt).
- Supplemental package `cargo check --locked -p dracon-sync` passed without warnings: [integrated-cargo-check.txt](integrated-cargo-check.txt). This is not the workspace Clippy gate.
- The source/configuration files recorded after these checks are identified in [qualified-fixture-source.sha256](qualified-fixture-source.sha256).

The fixture harness invokes the same strong `reclaim::attempt` transaction that the production entry point would use after qualification. It does not enable that entry point or add a live bypass. Hook/filter-free real-Git fixtures join their plumbing processes and use a test-only empty-family attestation. Separate real-host producer tests exercise actual transient-service/cgroup settlement and the identity-bound handoff. Test-only attestations and fault selectors are not compiled into production.

## Behavioral coverage

The output names tests demonstrating:

- Duplicate removal only after independent recovery of index, metadata and complete required objects; that record remains valid after ordinary main/recovery-store GC.
- Mixed removal after writing and indexing an exact main-missing-object sidecar and checking its enumeration, content/OID readback, types and sizes in an isolated ODB. A second standalone readback and relocation of the record away from the original repository also succeed.
- Deliberate corruption **after sidecar write/indexing** prevents removal and leaves original index and pack bytes unchanged.
- Original index mappings preserve raw non-UTF8 paths, spaces, executable and symlink modes; opaque metadata names/bytes survive.
- Source mutation after receipt, failed storage, failed ledger durability and injected pre-removal crash boundaries preserve the source. Already-published recovery records remain retained.
- Old live producer exclusion, an actual separate-process reclaimer holding the kernel fence, competing asynchronous reapers and uncertain unlocked legacy refusal.
- Preserve-marked sources survive the transaction, automatic/public cleanup and age-only apply paths; the protected-epoch regression covers the actor's early retention path.
- Packless sources require a valid recoverable index; invalid indexes and unknown subdirectories are preserved. No packless or publication shortcut remains.
- Age-only apply still refuses; fresh directories remain outside automatic scheduling.

## Remaining qualification

Corrected mutation/non-vacuity demonstrations, full workspace tests, strict workspace Clippy, release build, dependency-policy checks, final review and consistent-scope live accounting are still pending. No fresh live-drain baseline, live rescue or net-reclamation result is claimed by this checkpoint. Historical scans and unexplained disappearances must not be attributed to the corrected transaction.
