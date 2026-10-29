# Partial daemon follow-up after fleet-query timeout — 2026-10-29

## Bounded observations

- `dracon-sync-operator.service` was active at 00:49 UTC.
- The journal recorded the daemon committing two files in the `dracon-sync`
  checkout at 00:49:43 and `synced (late)` at 00:49:51. The subsequent local
  checkout was clean at `b2fc16c1434898fef78c847c8fa8cf88338574e7`, equal to its
  `origin/main` tracking ref; `gitlab/main` also named that commit. This is
  evidence of daemon-owned local commit/sync plus local tracking-ref agreement,
  not a fresh `ls-remote` check of every intended destination.
- In the selected 00:45:00–00:51:34 journal excerpt, the daemon's
  `browser-extensions-shared` unpushed count rose from 64 to 73 while new local
  commits continued. Each sampled publication message said another worker owned
  publication; no browser-extensions-shared sync completion appears in this
  excerpt. A `run-push-gates` Node process was present at one process snapshot
  with 10:49 elapsed, but its outcome was not observed. The existing browser
  hook designs describe long, bounded gates; this snapshot alone neither proves
  a stuck gate nor proves eventual publication.
- The journal reports an `endless-td` staged-epoch deadline expiry at 00:47:14,
  followed by `synced (late)` at 00:50:12. It separately reports a late
  `dracon-platform` sync failure at 00:48:21 whose ref outcome may require
  inspection; this selected interval contains no later parent-platform sync
  result. `trailing-drain` still reported one or two tasks running at 00:51:34.

The filtered journal excerpt is retained at
`daemon-followup-journal-2026-10-29.log`. A fresh `dracon-sync repos` query had
already timed out after 30 seconds without output. The daemon is making local
and late-sync progress, but the live backlog and in-flight work mean this is
not a clean-idle fleet observation. The full fleet state, direct advertised
remote refs for every repo, and the final gate outcomes remain unknown; no
manual push, state cleanup, or producer shutdown was performed.

**Next:** when host resource headroom returns, obtain a bounded full fleet status,
then separately verify the browser publication result, the parent-platform ref
outcome, and all intended advertised refs. Preserve active producers and the
recorded local work.

## Later bounded follow-up — stale parent ref lock (2026-10-29 ~09:59 UTC)

At 09:58:21 the daemon reported a late `dracon-platform` `update-ref` failure:
`.git/refs/heads/main.lock` already existed. A 09:59 read-only check found that
path was a zero-byte regular file owned by `dracon`, last modified at 02:27:25;
`fuser -v` found no holder, the matching Git-process query was empty, and the
scheduler showed the parent retrying rather than executing. This supports a
stale-lock diagnosis, not an active transaction.

Using the sanctioned `dracon-sync maintenance -- ...` wrapper, the daemon was
paused while a guarded check revalidated the file type, owner, zero size, age
(27,175 seconds), stable inode and absence of a holder twice, then removed only
that empty lock. The wrapper reported sync resumed. The lock remained absent on a later check,
but at 10:03:32 the daemon reported five consecutive `dracon-platform` sync
failures and a 15-minute backoff; the 10:03:51 scheduler showed a retry still
roughly 880 seconds away. The sampled interval contains no per-attempt error
explaining those failures and no post-repair parent ref result, so the stale-lock
removal is not yet shown to have restored publication. At 10:05:29 the scheduler
still showed the parent in backoff, with roughly 734 seconds before retry; the
lock remained absent in the 10:03 check. Do not attribute the remaining failures
to that lock or clear the retry state without a cause-specific qualification. No
broader fleet query or browser-ledger clearing was done.

## Later parent hook result (2026-10-29 ~10:07 UTC)

At 10:06:18 the daemon reported that `dracon-platform`'s post-commit hook
failed **after staged-epoch publication** with `STALE: products, games, music,
studio, tools, books, chat, jobs`. It subsequently committed additional parent
files, but no advertised parent ref was checked. At 10:07:48 the scheduler still
showed the parent retrying with a backoff; the earlier stale-lock removal has
not been shown to restore parent convergence. Do not infer the post-commit
failure is caused by the removed lock.
