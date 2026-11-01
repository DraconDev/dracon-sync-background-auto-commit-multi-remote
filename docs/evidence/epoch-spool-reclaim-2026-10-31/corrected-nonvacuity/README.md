# Corrected transaction: mutation/non-vacuity evidence

This covers the durable-recovery implementation, not the earlier presence-only prototype. It does not enable live reclamation or replace the workspace gates.

## Reproduce

```sh
python3 docs/evidence/epoch-spool-reclaim-2026-10-31/run-corrected-nonvacuity.py
```

The [driver](../run-corrected-nonvacuity.py) copies the tracked parent and three utility repositories into a new private cache directory. Runtime state and build outputs are excluded. It verifies copied inputs, uses a separate `CARGO_TARGET_DIR`, patches only copied files, restores bytes in `finally`, and keeps all artifacts. No source branch is moved, no live fixture/data is mutated, and no live helper binary is overwritten. Snapshot inputs are recorded in [snapshot-inputs.json](snapshot-inputs.json); commands, exits and timings are in [results.json](results.json) and [driver.log](driver.log).

Each case ran its named test successfully before mutation, failed the intended behavioral assertion with exit **101** after mutation, and passed with exit **0** after restoring its source. Compilation or timeout failure would have stopped the driver instead of counting as non-vacuity.

| Safety boundary deliberately broken in copied code | Observed failure | Restored observation |
|---|---|---|
| Classify all source OIDs as main-missing, including actual duplicates — [patch](duplicate-mutation.patch) | Duplicate fixture rejected classification `2` instead of `0`, not a Git/setup failure — [broken](duplicate-broken.log) | Named duplicate test passed — [baseline](duplicate-baseline.log), [restored](duplicate-restored.log) |
| Skip the entire unique-sidecar enumeration/content/OID readback boundary while retaining deliberate post-write corruption — [patch](rescue-verification-mutation.patch) | Preservation assertion failed because the transaction granted removal despite the corrupted sidecar — [broken](rescue-verification-broken.log) | Named post-write-failure preservation test passed — [baseline](rescue-verification-baseline.log), [restored](rescue-verification-restored.log) |
| Bypass both components of ownership exclusion: kernel-lock acquisition and settled-marker validation — [patch](ownership-mutation.patch) | Actual live child owner was no longer excluded; the assertion requiring refusal failed — [broken](ownership-broken.log) | Named old/live-owner test passed — [baseline](ownership-baseline.log), [restored](ownership-restored.log) |

The ownership mutation explicitly disables **both independent defenses**. Bypassing only the kernel lock while leaving the absent settled marker authoritative would still preserve the fixture; that would not demonstrate a vacuous live-owner test. Its mutated build emitted an unused-`Read` warning because marker parsing was removed, but compilation succeeded and the named runtime assertion failed. This warning is not a restored-tree quality result.

After restoring every mutation, the driver rebuilt the source-matching helper ([build](restored-helper-build.log)) and reran the entire spool suite with the host fixtures explicitly enabled: **40 passed, zero failed, zero ignored** ([output](restored-epoch-spool.log)). The original implementation/configuration checksums still matched afterward ([live-source-check.txt](live-source-check.txt)).

Full workspace tests, strict Clippy, release/dependency gates, final review and consistent-scope live rescue/accounting remain separate pending work. The destructive qualification flag remains false.
