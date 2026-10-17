# Hermetic daemon-worker enrollment qualification — 2026-10-17

## Scope and entry point

Run from the standalone crate (or pass the full script path):

```sh
timeout 420 bash scripts/qualify-daemon-bridge-e2e.sh
```

The bounded top-level driver runs three Linux Rust scenarios through the
production `sync::sync_repo` worker used by the daemon. That includes cleaning,
new candidate discovery, real measured routing, child runner execution,
publication verification, directory-expanded staging and an actual Git commit.
It then runs the complete Node component/runner/fault-recovery suite.

This replaces qualification based on fake published verdicts or mere existence
of old manifests. Protocol-stub unit tests remain useful parser tests but are
not new-enrollment evidence. The old operational live-deployment/enrollment
scripts are not invoked or claimed as evidence by this hermetic gate.

The fixture imports the real `createRunner`, publisher, bindings, adapters and
restorer. Only `transportFactory` storage primitives are substituted with a
private temporary filesystem store and controlled failures. Repository JSON
cannot select this factory. The wrapper's published/refused verdict is the
real runner's result, not fabricated proof. Its --version is the real protocol
VERSION. Fixture files are never installed as production artifacts.

No test file launches Cargo/Node test suites. Only this top-level shell driver
launches them. Fixture subprocesses run the runner library, Git, or flock—not
the suite. The driver requires all three exact scenario names and their pass
summary. The Node driver uses TAP, all 157 current tests, required named
scenarios and no failures/skips/cancellations/todos; additional tests can grow
without a stale hardcoded exact count. Missing prerequisites fail nonzero.

## Contract-to-artifact evidence

All three daemon scenarios are in `src/sync/bucket_qualification_tests.rs`.
Storage injection/setup is `scripts/fixtures/bucket-daemon-transport.mjs`.

| Requirement | Executed assertion |
| --- | --- |
| New enrollment, not pre-existing state | Each scenario begins with no manifest, descriptor, registry/index or remote objects. Two newly generated canonical 512x512 PNGs are above the real 1 KiB routing boundary and are NEVER manually staged. The real worker commits newly published metadata; HEAD changes and committed manifest/descriptor bytes equal verified worktree bytes. |
| Reference-only commit | Both source images stay byte-identical and absent from Git tracking. Publication lock is untracked. An unrelated new text file in the SAME public directory is committed, proving suppression does not discard the whole directory. |
| Storage outage | HEAD probe throws before any PUT: worker returns Blocked, manifest remains absent, HEAD/index and local bytes unchanged. |
| Partial upload | First object persists and passes readback; second upload fails: worker Blocked, one stored object, no manifest, no commit, all source bytes intact. |
| Retry and idempotence | Healthy retry uses existing verified remote bytes, finishes enrollment with exactly two successful stores total; next worker cycle preserves HEAD, manifest/index bytes and store count. |
| Concurrent manifest | Storage readback injects an independent writer after the initial absent-manifest snapshot. Worker refuses; independent manifest bytes, HEAD/index and sources remain intact. Even a subsequent healthy retry must not overwrite that independent authority. |
| Fresh consumer and failure exits | A separate local Git clone has metadata but no materialized PNGs. Corrupt remote bytes cause the real restore child to return nonzero/refused with final files absent. Repairing the fixture object allows restore retry; both restored files equal original bytes AND digests. Reader has no PUT primitive; store count and publisher binding registry stay unchanged. |
| Failure cannot silently qualify | A parent-run negative driver probe supplies a zero-test Cargo result: driver exits 1 for missing required scenarios. Genuine fault verdicts are refused/nonzero in the runner fixture and map to Blocked, not Synced, in the worker. |

## Production gaps found by the stronger harness

The initial real-worker tests failed rather than accepting a fake success.
Two in-scope integration fixes were necessary:

1. `bucket_runner_bridge::discover_candidates` only used tracked ls-files.
   New untracked files were absent from the sealed candidate set, so no fresh
   enrollment could occur. Enumeration now uses bounded cancellation-owned,
   filter-free NUL-safe cached/untracked Git lists. It honors Git ignores,
   the untracked opt-out/excludes, global+repo auto-commit exclusions, file/dir
   patterns and stage-size limits. Symlinks, nested Git roots and non-media
   files cannot enlarge discovery. Over-limit scans refuse rather than
   silently truncating. Narrow assetRoot filters retain repository-relative
   paths; materialization suppression no longer double-prefixes them.
2. Libgit2 can report an untracked directory rather than individual files.
   Suppressing materialized paths only in the initial partition was insufficient:
   later expansion staged PNGs and the existing index guard correctly refused
   the commit. Exact path/ancestor suppression now also runs AFTER expansion,
   before git add. It uses HashSet ancestor lookups, not quadratic set scans.
   The verified manifest's sibling publication lock is also suppressed.
   The final staged-index guard is unchanged and still rejects independently
   staged raw bucket sources. No existing tracked assets are retired.

A focused discovery test additionally covers space-containing filenames,
Git ignores, both operator exclusion layers, untracked opt-out, file patterns,
symlinks, independent nested repos and non-dot root paths. Existing protocol
and measured-payload regressions remain green.

## Bounded checks and limitations

- `timeout 420 bash scripts/qualify-daemon-bridge-e2e.sh`: passed; three actual
  worker scenarios plus 157 Node tests, zero failures.
- `timeout 150 env CARGO_BUILD_JOBS=1 cargo test -p dracon-sync --locked bucket_runner_bridge -- --test-threads=1`:
  25 passed, zero failures.
- `timeout 120 env CARGO_BUILD_JOBS=1 cargo test -p dracon-sync --locked bucket_qualification_daemon -- --test-threads=1`:
  3 passed, zero failures (also re-run after the final fixture policy adjustment).
- `timeout 120 env CARGO_BUILD_JOBS=1 cargo test -p dracon-sync --locked test_stage_existing_files -- --test-threads=1`:
  11 legacy directory-expansion/staging regressions passed, zero failures.
- `timeout 120 env CARGO_BUILD_JOBS=1 cargo clippy -p dracon-sync --locked -- -D warnings`:
  passed without warnings/errors.
- `timeout 20 node scripts/build-bucket-runner.mjs --check`: dependency closure
  verified; production runner source/bundle is unchanged by storage injection.
- Shell syntax and `git diff --check`: passed.

Evidence logs and changed-source checksums are under
`evidence/bucket-daemon-qualification-2026-10-17/`. Fresh independent source
review found no blockers; reviewer execution tools were unavailable, so all
runtime checks above were parent-run, not independently re-executed.

Fixture Git uses the operator/CI identity unchanged. Hooks are isolated only
inside temporary repositories and auto-hardening is disabled only in fixture
policy, as in other hermetic sync tests. No live hook settings, watch roots,
credential files, namespace permissions, daemon service or installed binary
were changed. All fault modes and object corruption affect temporary storage
only. There is no live S3/OVH transport or scheduler/backoff timing claim.

Full workspace/release gates, production deployment, real OVH readback and
autonomous scheduled live retry remain separate queued items. Hermetic worker
success is deliberately not presented as an installed/live production receipt.
