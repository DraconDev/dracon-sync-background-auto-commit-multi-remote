# Objection 1: auto commits run the configured hook chain

## What was wrong

`commit_staged_owned` invoked `git commit --no-verify`, and the regression added
alongside it asserted that the repository's `pre-commit` hook must NOT run.
Git history shows the bypass was introduced inside this goal, not inherited:
commit `3287891` created the helper with a plain `git commit`, and `abbb85f`
(04:42) added `--no-verify` and rewrote the test to match. So the report's "all
hooks"/"no hook bypasses" claim was contradicted by this daemon's own code —
the auditor was right.

## The fix

`commit_staged_owned` now runs plain `git commit -m <message>`; no `--no-verify`
anywhere on a production commit path (verified by scanning every non-`cfg(test)`
line in `src/`):

```
$ (scan for `no-verify` outside #[cfg(test)] modules)
src/sync.rs:5108  /// `--no-verify`. The in-process gates ...   <- prose only
```

Every remaining `--no-verify` hit is inside a `#[cfg(test)]` module, plus prose.
`test_helpers` (which carried the one compiled-in `--no-verify` primitive) is
now itself `#[cfg(test)]`-gated in `main.rs`, and the RELEASE BINARY contains
zero `--no-verify` literals (verified with `grep -c` on
`target/release/dracon-sync`), so no production commit path suppresses a hook.

The first version of this document claimed the scan covered "every
non-`cfg(test)` line" when `test_helpers.rs` was in fact compiled — that claim
was wrong and is corrected here.

### Storage-sealed commits drive the COMPLETE commit lifecycle

The auditor was right that running only `pre-commit` is not Git's contract. A
`git commit` executes four hooks, and a repository can legitimately put its
policy in any of them:

```
pre-commit argv=[]
prepare-commit-msg argv=[.git/COMMIT_EDITMSG message]
commit-msg argv=[.git/COMMIT_EDITMSG]
post-commit argv=[]
```

(measured with a probe repository, not assumed). `commit-msg` may also REWRITE
the message, and Git commits the rewritten text.

`run_sealed_commit_hooks` now reproduces that sequence around the libgit2 seal —
`pre-commit`, `prepare-commit-msg <msgfile> message`, `commit-msg <msgfile>`,
the seal, then `post-commit` only after a real commit — reading `COMMIT_EDITMSG`
back so the seal commits what `commit-msg` approved. Each hook is a bounded
owned process group. The ordinary `git commit` path runs none of this itself, so
no hook fires twice.

### Storage-sealed commits invoke the same chain

A storage-guarded commit is sealed through libgit2 (`storage/mod.rs`
`repository.commit`), which executes NO hooks. That was a second bypass on a
production auto-commit path, found by the fresh-context reviewer after the CLI
fix. Both dispatch sites (steady state and empty-repo bootstrap) now call
`run_configured_pre_commit` — `git hook run --ignore-missing pre-commit` —
before sealing. `--ignore-missing` is deliberate: a repository with no
`pre-commit` must not have every auto commit fail. A refusal aborts the pass
with the index and worktree intact.

Failure semantics are deliberate: a refusing hook makes `run_git_with_timeout`
return `Err` carrying the hook's own stderr. The sync caller maps that to
`ApplyOutcome::Failure` — the index and worktree stay intact, nothing is
published, the failure remains in the evidence, and the next pass retries. A
per-repo hook can therefore delay a commit but cannot lose an edit or be
reported as a successful commit. The in-process gates (bucket forward-only
guard, immutable-storage transaction, exclusions, size limits) still run BEFORE
the commit, so hooks are additional policy, never a replacement for them.

## Regressions (and proof they are not vacuous)

`src/sync/owned_commit_tests.rs`:

* `a_sealed_commit_runs_the_full_commit_hook_lifecycle_in_git_order` — asserts
  the ORDER (`pre-commit`, `prepare-commit-msg`, `commit-msg`, `post-commit`),
  the ARGUMENTS (`prepare-commit-msg` receives `<msgfile> message`;
  `commit-msg` receives the message file; the other two receive nothing), and
  that the seal commits the message `commit-msg` rewrote.
* `a_refusing_commit_msg_hook_blocks_a_storage_sealed_commit` — a refusing
  `commit-msg` blocks the seal, its refusal reaches the caller, no root commit
  is created and the work survives.
* `the_ordinary_commit_path_runs_each_hook_exactly_once` — all four hooks fire
  once per commit on the non-sealed path, so nothing is duplicated.
* `a_refusing_hook_also_blocks_a_storage_sealed_commit` — the seal path
  consults the chain (refusal propagates with its message) and an accepting hook
  is a no-op that does not itself commit.
* `a_repository_without_commit_hooks_still_commits` — no configured hook is not
  an error, so hookless repositories keep auto-committing.
* `owned_commit_runs_the_configured_hook_chain_and_preserves_worktree_edits` —
  an accepting `pre-commit` must be seen to RUN (marker file created), HEAD must
  advance, the staged snapshot must be committed, and the operator's unstaged
  edit must survive.
* `a_refusing_commit_hook_blocks_the_auto_commit_and_keeps_the_work` — a hook
  that exits 1 must fail the commit, its message must reach the caller, HEAD
  must not advance, the change must still be staged, and the unstaged edit must
  survive.

Each lifecycle stage is proven to bite, by reverting it and watching the
regression fail:

```
pre-commit only (old behaviour):
  a_sealed_commit_runs_the_full_commit_hook_lifecycle_in_git_order FAILED
    hooks must run in Git's order, got: pre-commit argv=[]
  a_refusing_commit_msg_hook_blocks_a_storage_sealed_commit FAILED
    a refusing commit-msg must fail the sealed commit: true
post-commit not invoked:
  a_sealed_commit_runs_the_full_commit_hook_lifecycle_in_git_order FAILED
    hooks must run in Git's order
```

Re-introducing the `--no-verify` bypass proves the CLI tests bite:

```
test ...owned_commit_runs_the_configured_hook_chain_and_preserves_worktree_edits FAILED
  the configured pre-commit hook must run on an auto commit
test ...a_refusing_commit_hook_blocks_the_auto_commit_and_keeps_the_work FAILED
  a refusing pre-commit hook must fail the commit: ()
test result: FAILED. 2 passed; 2 failed
```

With the fix restored: `test result: ok. 4 passed; 0 failed`.

---

# Objection 2: remote-ref advancement under the FINAL binary

## What was wrong

The ref-advance evidence named the earlier `7aaaea03…` revision, and the
`35ee77d6…` snapshot showed only phases and pending fields. Status fields are
not proof of publication.

## What was measured

`refs/live-ref-observe.py` (archived beside its output) records, per repo, local
HEAD, each remote's advertised branch ref via `git ls-remote`, and the object
type of that ref via `git cat-file -t`. Every command is a read: no push, no
fetch, no ref update, no config change, no state repair.

The harness also hashes the LOADED executable of the running daemon
(`/proc/<MainPID>/exe`), so every sample is tied to a specific revision rather
than to installation metadata.

## Result — FINAL installed revision

The final artifact after the full-lifecycle fix is
`15054b66fdf6905c727195b7da99e2b7cfb7698260e2f9c94c275f9b28e94952`, installed at
13:07:53Z (previous `7e0ea08a…` backed up). 12 read-only samples, every one
recording the same loaded executable and PID:

```
/proc/3935918/exe = 15054b66fdf6905c727195b7da99e2b7cfb7698260e2f9c94c275f9b28e94952
release artifact  = 15054b66fdf6905c727195b7da99e2b7cfb7698260e2f9c94c275f9b28e94952
```

**18 distinct remote-ref advancements**, every one resolving to
`object_type == "commit"`:

```
.dracon  origin/gitlab  086d84cee8 -> 2f589ced86 -> 895e1a9ba3 -> 5f6d0c1a2f
                        -> 2066efb55a -> 4aed42d3c2 -> b071cf7494
                        -> 5055faf4a4 -> f069fdeca3 -> b80c9f21f3     (commit)
.dracon  github         follows one sample behind origin/gitlab each round
```

Computed from `refs/live-ref-samples-final.json`, not asserted:

* all 12 samples report the same loaded digest `15054b66fdf6` and PID 3935918 —
  the final binary;
* every recorded remote tip is a real commit object (0 exceptions);
* `dracon-utilities` stayed converged the whole window while `.dracon` advanced
  9 times, i.e. one repository's publication never blocked another's;
* per-mirror ordering is visible: `github` lags `origin`/`gitlab` by one sample
  then catches up — independent per-mirror publication, not one global slot.

Honest limits, stated rather than smoothed over:

* `browser-extensions-shared` carries a real, pre-existing backlog, and the
  cause is identified rather than guessed: its own `pre-push` refuses because
  `youtube-filter/TEST` pins the literal source text of
  `generate_store_assets.py` while another agent was editing that file
  (commits at 11:43/11:44 during an 11:43:02 run). Re-run on settled files it
  passes (899/899, typecheck 0). A live-edit race, not a daemon defect; a
  genuinely gated backlog, not converged.
  Details: `browser-extensions-shared-gate-diagnosis.md`.
* `dracon-platform` shows local-ahead (`!`) throughout this window, i.e. it had
  unpushed work at those instants and was NOT converged.
* These samples prove ref advancement and object types for the listed
  repositories. They do not claim every fleet repository converged.

Basis for "real autonomous progress": the commits above came from the daemon
acting on live repository edits. No commit, push, repair, hook bypass or state
change was performed by hand; every harness command is a read (`rev-parse`,
`remote get-url`, `ls-remote`, `cat-file`, `sha256sum`).

## What the receipt does and does not cover

The archived receipt's fixtures are NOT storage-guarded, so their commits take
the ordinary `git commit` path. Its lifecycle assertions therefore prove the
ORDINARY path runs `prepare-commit-msg`/`commit-msg`/`post-commit` in Git's
order for every daemon commit (`b_lifecycle_cycles: 2` — B made two commits, and
each produced one complete cycle). They do NOT exercise the sealed path; that
one is covered by the unit regressions above, which drive
`bootstrap_empty_repo_commit` on a genuinely guarded fixture. Both binaries in
this goal pass the receipt, which is consistent: the receipt's own scope never
included the seal.

## The receipt now proves hook behaviour on the installed binary

`isolated/receipt.json` (harness archived beside it) grew a third repository
whose `pre-commit` REFUSES, and asserts:

| assertion | result |
|---|---|
| accepting hook RUNS on B's daemon commit | `b_precommit_hook_ran: true` |
| refusing hook RUNS on C | `c_precommit_hook_ran: true` |
| refusal reaches the log | `c_commit_hook_refusal_reported: true` |
| HEAD did not advance | `c_head` == probe baseline `332a480d0b` |
| nothing was published | `c_remote_refs: ""` |
| the operator's work survived | `c_work_preserved: true` |

The daemon's own log contains the hook's message twice (`warden: synthetic
refusal`), so the refusal is reported rather than swallowed. A/B assertions are
unchanged and still green: A blocked, B published, A took a second real commit,
A converged on its newest tip (`a_remote_tip == a_newest_tip`), both repositories
holding their own operation+publication leases, `ok: true`,
`shutdown_unclean_lines: []`, no orphans.

## The hook assertions are not vacuous

The same archive harness was pointed at the PRE-FIX binary still on disk
(`d1b36d0f596195737f98…` — an intermediate pre-fix build, NOT the immediately
previous install, which the deployment receipt records as `35ee77d6…`; both are
version 0.113.95). It FAILS,
with the assertion that matters:

```
AssertionError: the daemon suppressed B's configured pre-commit hook
b_precommit_hook_ran = False
```

So the receipt distinguishes a daemon that bypasses commit hooks from one that
runs them: `ok: false` on the bypassing revision, `ok: true` on the fixed one,
with the C-refusal assertions never even reached on the old binary. Receipt and
daemon log archived under `isolated/nonvacuity/`.

## Side effect found and fixed by the real hooks

Turning hooks on immediately exposed a fixture that had been relying on the
bypass: `bucket_assets::tests::daemon_commits_verified_reference_not_raw_bytes_and_recovers_from_failure`
failed with the ambient warden hook's own message
("Warden filter missing from .gitattributes"). That is the hook working. The fix
is fixture isolation, not daemon behaviour: the fixture sets a LOCAL
`core.hooksPath` to an empty directory, the same way any out-of-warden-scope
repository does — the daemon still runs whatever hooks the repo configures.
