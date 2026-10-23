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

Re-introducing the bypass proves both tests bite:

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

12 samples over ~6 minutes, every one recording the loaded executable of the
running daemon:

```
/proc/{1375344}/exe = 9eb4965d54dd0b9b61ada489d375c12766bb09a21fcc3c4c44ba197d6b66b5ed
release artifact     = 9eb4965d54dd0b9b61ada489d375c12766bb09a21fcc3c4c44ba197d6b66b5ed
```

That is the revision installed at 09:15:15Z after the hook fix (the receipt also
records the replaced `35ee77d6…` binary and the backup path). **33 distinct
remote-ref advancements**, every one resolving to `object_type == "commit"`:

```
.dracon                   origin/gitlab/github  f975ed8731 -> 9681f44942
                          -> f416f8dbe6 -> 596c711652/87caf91881 -> 15f752fdc9
                          -> 2f5f4a1a7f/7b7fa2be4f -> aaf4981a78 -> 009df31d7c
                          -> a4311b51fb                                  (commit)
browser-extensions-shared origin/github         e2830b3a4b -> 390a045d23 -> a3d99fb282
browser-extensions-shared gitlab                2b887bf554 -> 390a045d23 -> a3d99fb282
dracon-platform           origin/github/gitlab  31c481b9f7 -> 5f3aab1e41
dracon-platform           origin                5f3aab1e41 -> 4d4c86f075
```

Computed from the archived JSON (`refs/live-ref-samples-final.json`), not
asserted:

* all 12 samples report the same loaded digest `9eb4965d54dd` — the final
  binary, not an earlier one;
* every recorded remote tip is a real commit object (0 exceptions);
* `dracon-utilities` stayed converged throughout while the active repositories
  advanced, i.e. one repository's publication never blocked another's;
* `.dracon`, `dracon-platform` and `browser-extensions-shared` each advanced
  under the SAME revision that the live daemon was running, several
  independently within the same sampling window;
* mirrors lag and then catch up (visible per-remote `!` marks that later become
  `=`), which is per-remote publication rather than a single global slot.

Honest limits, stated rather than smoothed over:

* `browser-extensions-shared` still has a real, pre-existing backlog: its
  `gitlab` mirror repeatedly trails `origin`, and its log shows the genuine
  cause — the repository's own `pre-push` policy refusing the push
  ("asset-guard / size budget / secret scan / history guard"), which the daemon
  correctly classifies as a POLICY refusal rather than a network failure. That
  gate is product policy, and repairing it is out of scope here (no hook bypass
  is permitted), so this repository is reported as a gated backlog rather than
  as converged.
* These samples show ref advancement and object types. They do not claim every
  fleet repository is converged.

Basis for the "real autonomous progress" clause: the commits above were produced
by the daemon from live repository edits. No commit, push, repair, hook bypass
or state change was performed by hand for this record; every command in the
harness is a read (`rev-parse`, `remote get-url`, `ls-remote`, `cat-file`,
`sha256sum`).

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
