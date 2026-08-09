# dracon-sync v0.113.48 — 2026-08-09

Detached-HEAD push stuck on bare `HEAD` refspec — fix the bare-HEAD escape hatch
that left a 197-file commit wedged for ~50 minutes.

## What broke

Three push sites — `push_with_transport_fallbacks` (`src/git/push.rs:97`),
`push_with_retries` (`src/git/push.rs:165`), and `multi_remote::push_to_remote`'s
retry loop (`src/git/multi_remote.rs:609`) — used the bare refspec `HEAD` when
`current_branch(repo) = Some(branch)`. Only the detached-head fallback used the
fully-qualified `HEAD:refs/heads/main`.

On a detached worktree, `HEAD` is a commit SHA. git's push parser sees
`HEAD` as the `<src>` and rejects it with:

    error: The destination you provided is not a full refname (i.e.,
    starting with "refs/").
    hint: The <src> part of the refspec is a commit object.
    fatal: ref HEAD is not a symbolic ref

The HTTPS fallback at `push.rs:139` already used `HEAD:refs/heads/<branch>` and
worked, so the daemon self-recovered when the SSH retries gave up — but the
50-minute stall was real.

## Live evidence

`pi-goal-loop-audit` had a 197-file commit ahead on 2026-08-09. gitlab SSH push
started at 10:05:42, failed with the refspec error, retried every ~2 minutes for
the next 50 minutes, then succeeded via the HTTPS fallback path at ~10:55. The
repo's `pushing 47m` indicator misled the operator into thinking it was
actively in flight.

## Fix

Always use the fully-qualified refspec `HEAD:refs/heads/<branch>` when a branch
is known — works for both attached and detached worktrees. The detached-only
fallback to `"main"` is preserved as a last resort.

Three sites patched:

- `src/git/push.rs:97` (`push_with_transport_fallbacks`)
- `src/git/push.rs:165` (`push_with_retries`)
- `src/git/multi_remote.rs:609` (`push_to_remote` retry loop)

## Tests

Two new regression tests in `src/sync.rs`:

- `test_push_succeeds_with_detached_head` — builds a repo, detaches HEAD at
  push time, verifies the fully-qualified refspec push lands on origin.
- `test_refspec_format_is_always_qualified` — pins the contract that no push
  refspec is the bare `HEAD` form (tripwire for future reverts).

One existing test updated in `src/git/mod.rs`:

- `test_push_to_named_remote_https_fallback_failure_still_retries_ssh` —
  pre-fix, this test relied on the retry loop using bare `HEAD` to slip past
  the test's fake-git. Post-fix, the retry loop uses the same qualified
  refspec as the SSH attempt and HTTPS fallback. The test now asserts the
  new (deterministic) failure mode: the retry loop IS REACHED (not
  short-circuited), it just fails the same way as the SSH attempt instead of
  accidentally succeeding via the bare-HEAD escape hatch.

**1240 passed, 9 ignored** (+2 regression tests, −0 broken). Clippy
`-D warnings` clean (only the 6 pre-existing daemon.rs / report.rs warnings
remain — none in the touched files). `cargo deny check` clean.

## Operator action

`dracon-sync maintenance -- bash -c "scripts/release.sh 0.113.48 --yes"`

The fixture check (release.sh step 6) installs the packaged artifact with
fresh dependency resolution and runs the untracked-count fixture. Real
`cargo install dracon-sync --version 0.113.48` from crates.io should be the
verifying install for an outside observer.
