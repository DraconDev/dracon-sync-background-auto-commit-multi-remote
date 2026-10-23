# Final lifecycle qualification

This is the current qualification record. Earlier rounds in `progress.md`,
`live-observation.md` and `hook-and-ref-evidence.md` are historical; their
artifact hashes, sample counts and test totals must not be treated as current.

## Remaining objection and durable correction

The auditor correctly found that running pre-commit alone before a libgit2
commit bypassed prepare-commit-msg, commit-msg and post-commit.

The sealed auto-commit dispatch now runs:

1. pre-commit, no arguments;
2. prepare-commit-msg, message-file path and `message`;
3. commit-msg, message-file path;
4. re-read the approved message; abort if empty;
5. verify storage and create the immutable commit object WITHOUT advancing HEAD;
6. bounded native `git update-ref HEAD <commit> <expected-old-tip>` while holding
   the storage commit lock, honoring reference-transaction hooks and refusing
   concurrent HEAD changes;
7. post-commit, no arguments, only after a successful HEAD update. As with Git,
   post-commit failure is logged but cannot undo the existing commit.

Hook processes use owned process groups, finite operation deadlines and
GIT_EDITOR=: (the no-editor commit convention). Missing hooks are allowed. A
vanished storage binding blocks the pass instead of falling back to a native
commit that would run the same hooks twice. Unusable bindings fail closed.

The ordinary path calls native `git commit` once, without running the sealed
prelude. The low-level libgit2 direct-commit test helper is cfg(test)-only;
production sealed paths prepare the object and advance refs through native Git.
No global push-count limit has been reintroduced.

## Behavioral coverage

`src/storage/tests.rs` drives the real storage-bound bootstrap dispatcher:

- a_sealed_commit_runs_the_full_commit_hook_lifecycle_in_git_order: ordered
  hook log, exact arguments (no untested extra args), and amended message is
  what gets committed;
- a_refusing_commit_msg_hook_blocks_a_storage_sealed_commit: refusal reaches
  caller, no root commit, operator work retained;
- sealed_commit_rejects_a_message_emptied_by_commit_msg: zero-exit hook that
  empties the message still aborts, rather than restoring the original text;
- sealed_commit_honors_a_refusing_reference_transaction_hook: native Git's
  prepared-phase refusal prevents HEAD update and post-commit; work retained;
- existing raw-byte/unbound-storage guard regressions remain enabled.

`src/sync/owned_commit_tests.rs` now drives real sync_repo for the ordinary
exactly-once regression. A duplicate prelude in the dispatcher, not merely in
its private commit helper, would change the counts.

Earlier non-vacuity experiments are preserved in `hook-and-ref-evidence.md`:
pre-commit-only fails the ordered-lifecycle and commit-msg refusal regressions;
omitting post-commit fails the ordered-lifecycle equality. Tests retain semantic
checks rather than pinning incidental file counts.

Fresh-context reviewer inspected the current named seams and tests and reported
no issues or blockers. It did not execute tests or review commit attribution;
those are not claimed as independent execution evidence.

## Final gates and artifact

All commands exited zero on the final source:

- cargo test --workspace --locked: 2582 passed, 0 failed, 25 ignored
  (`workspace-suite.txt`);
- cargo clippy --workspace --locked --all-targets -- -D warnings
  (`clippy-final.txt`);
- cargo build --release --locked (`release-build.txt`);
- cargo deny check: advisories, bans, licenses, sources OK (`deny-final.txt`).

Qualified artifact SHA-256:
`481e3740cf26281ba6b48e3494d903065821a2728d8b4466d5d69b512383a883`.

Installed with atomic replace, previous executable backup, pidfd-targeted
SIGTERM and the existing Restart=always service. Pause is cleared in finally.
`installed-receipt.json` verifies the new /proc/2507147/exe digest, not just the
installed pathname. No service stop was used.

`isolated/receipt.json` was generated AFTER installation by executing
/home/dracon/.local/bin/dracon-sync. Its recorded digest matches the loaded
artifact above. It proves A remains blocked while B publishes, another real A
commit lands during the blocked push, A's remote eventually equals its newest
commit, and the published object has type commit. Both repositories have their
own ownership leases. Hook lifecycle cycles on B are complete and ordered;
C's refusing hook leaves HEAD at baseline and publishes nothing. Shutdown has
no unjoined-task warning and no orphan processes. Receipt ok=true.

The receipt's repositories are not storage-bound: its lifecycle assertions
cover the ordinary path. Actual storage-bound lifecycle coverage is in the
real-dispatch Rust tests above, not falsely attributed to that receipt.

## Autonomous live observation on the loaded final revision

`refs/live-ref-samples-final.json` contains 11 read-only samples on PID 2507147,
all with the same final loaded digest. `refs/native-ref-final-summary.json`
derives 27 per-remote ref changes, zero non-commit tips and zero failed ref
queries. Both .dracon and dracon-sync advance on the daemon's own commits;
dracon-sync's final advertised tips equal its actual local HEAD across
origin/github/gitlab, while other sampled repositories remain independent. Every advertised nonempty
tip is checked with git cat-file -t. Query failures are recorded explicitly,
not treated as empty/successful refs.

Latest status (`live-repos-final.json`): 36 repositories, 28 ok, 4 active,
4 warn, 0 concern, 0 reported failures. These are observations, NOT proof of
fleet-wide convergence: browser-extensions-shared still has 173 unpushed
commits and dracon-platform 24, both currently publishing. Their refs did not
advance in this sampling window. They are not called healthy or converged.
The older diagnosed youtube-filter refusal and concurrent asset edits are
recorded in `browser-extensions-shared-gate-diagnosis.md`; that historical cause
must not be assumed to explain every later pending commit.

The journal during this window records real autonomous commits in
browser-extensions-shared, .dracon, db-gateway and other repositories. Running
publication does not prevent those local commits. No live manual push,
repository-state repair, history rewrite, force push, checkout deletion,
aud-vis rewiring, hook bypass, assertion weakening or identity/trust/secret
policy change was used to clear a backlog. Only isolated fixture repositories
were pushed by test code. The volume refusal's previously established worker
resource diagnosis and bounded worker repair remain unchanged.
