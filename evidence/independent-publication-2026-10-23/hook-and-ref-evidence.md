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

Remaining `--no-verify` hits are fixture setup in `#[cfg(test)]` modules
(`test_helpers`, `report` fixtures, per-test `git commit` seeding) and one
historical prose mention. No production commit suppresses a hook.

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

## Result

12 samples over ~6 minutes, all recording
`/proc/3685181/exe = 35ee77d64ee76c5df9ad861df25ca4ca937dfd9426d45174d3930bfb21050ffa`
— the final qualified binary. **30 distinct remote-ref advancements**, every one
a real commit object:

```
.dracon               gitlab/origin/github  fb1f3171bd -> 041286daac -> a02b80a044
                                            -> 62259530d0 -> 90ccb33d49 -> c6ead7f72a
                                            -> a09753ecb6 -> 458660ba7f -> 3c7f972a0a
                                            -> 8b05d36321 -> 242132324b     (all type=commit)
browser-extensions-shared github/origin     8ec71cc086 -> 81f3c5a123 -> e85ab60414
browser-extensions-shared gitlab            d2a43e7fa3 -> 81f3c5a123
dracon-platform          origin/github/gitlab  ca80c4c478 -> a27b498be1
```

Verified properties, computed from the archived JSON rather than asserted:

* every recorded remote tip resolves to `object_type == "commit"` (0 exceptions);
* the sampled remote refs for `dracon-platform`, `.dracon` and
  `dracon-utilities` equal local HEAD, i.e. the daemon actually converged them;
* `browser-extensions-shared` shows the honest lag this objective cares about:
  its MIRROR (`gitlab`) trailed at `d2a43e7fa3` while `origin` had already
  reached `81f3c5a123`, then caught up — a per-remote advance, not a false
  "everything synced".

Mirror-order effects are visible too: `.dracon`'s `github` leg updated one
sample after `origin`/`gitlab`, which is what independent per-remote publication
looks like from outside.

Basis for the "real autonomous progress" clause: these commits were produced by
the daemon from live repository edits (`.dracon` advanced 11 times in 6 minutes
of normal fleet activity). No commit, push or repair was performed by hand for
this record.
