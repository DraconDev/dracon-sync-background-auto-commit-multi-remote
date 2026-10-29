# Polis 15-minute refusal loop — root cause and repair (2026-10-29)

## Symptom

`dracon-platform/web/games/wip/polis` refused every staged epoch, five times,
then backed off 15 minutes, forever. Journal (14:54–14:59 UTC):

```
⚠️ polis local commit during publication failed (publication unaffected):
   refused epoch message retained at .../dracon-epoch-message-<id>:
   git pre-commit failed ... status exit status: 1: 0 orphan bun test/verify-release processes
pre-commit: bucket high-water/forward-only guard blocked this commit
⚠️ polis exceeded max failures (5), backing off 15 min (will re-probe)
```

`❌ polis exceeded max failures (5), backing off 15 min (will re-probe)`` is
the daemon behaving correctly. The refusal itself is a real guard decision, not
a daemon bug: the repository genuinely measured above the configured high-water
limit, so the hook was right to block and the daemon was right not to retry
faster than the 15-minute backoff.

The refusal text was previously unclassified (the retained epoch directory holds
only the generated `COMMIT_EDITMSG`, never hook stderr). Running the effective
hook directly reproduced it exactly:

```
"code": "BUCKET_STRATEGY_HIGH_WATER",
"projectedBytes": 2365393383,
"limitBytes": 2344615936,
"remainingBytes": -20777447,
"limitingRemote": "github",
"errors": ["projected pushable size 2365393383 is at or above provider limit 2344615936"]
```

**The reachable history was genuinely 20.8 MB over GitHub's 2.18 GiB limit.**

## Why it was over

`pi-session-*.html` is un-ignored fleet-wide (`!pi-session-*.html` in
`.gitignore`, present in the parent and every game). That is intentional — the
operator's commit-all policy — so the file is legitimately tracked.

The problem is that one session export
`pi-session-2026-10-26T11-05-09-981Z_01a177e2-….html` was rewritten and
re-committed 14 times over eleven minutes (14 distinct blobs, each ~75 MB).
Because they landed as **loose** objects, Git never delta-compressed the
near-identical copies:

```
loose_bytes = 1,864,982,930   (1.86 GB)
pack_bytes  =   911,024,780   (0.91 GB)
```

`git count-objects -v` reported `count: 4606 / size: 1834196 KiB` — 4606 loose
objects holding 1.83 GB, versus 94325 packed objects holding 889 MB.

This was not unreachable garbage: all 4606 loose objects were reachable
(`comm` of loose names against `--batch-all-objects` returned 0 orphans), so
`git gc --prune=now` reclaiming "junk" would have been the wrong diagnosis. The
bytes were real history that simply had not been packed.

## Repair

Repack the object database, exactly as sanctioned. No history rewrite, no
force-push, no deletion:

```
dracon-sync maintenance -- git -C .../web-games-polis gc --quiet
```

Deliberately routed through `dracon-sync maintenance` per the daemon-quiesce
policy (pause → run → always resume), never a bare `systemctl stop`.

Result (`pre-repack-objects.txt` → `post-repack-objects.txt`):

| | loose | pack | total |
|---|---|---|---|
| before | 1.86 GB | 0.91 GB | 2.78 GB |
| after | 0 | 2.08 GB | 2.08 GB |

`in-pack: 98858`, `size-pack: 2034483 KiB`, `count: 0`. The 14 near-identical
blobs now delta-compress; ~700 MB of the on-disk object database is gone and
the *reachable stored* size dropped from 2.253 GB to 1.735 GB.

Guard re-run (`guard-post-repack.json`):

```
ok=True code=OK
raw=6.867GB stored=1.735GB pushable=1.822GB limit=2.345GB remaining=522.3MB
```

522 MB of headroom instead of a 20.8 MB deficit.

The real effective hook was then executed directly, in the live worktree, on the
real staged epoch (`0 orphan bun test/verify-release processes`, `"ok": true`,
`"code": "OK"`), and exited 0. The refusal path is closed.

## Classification

- **Was this a daemon defect?** No. The guard, the refusal, the retained epoch
  message and the 15-minute backoff were all correct. The daemon's only fault
  was in *reporting*: it logs the retained message path but never the hook's
  stderr, which is why this sat unclassified through many checkpoints. That
  reporting gap is worth a follow-up, not a convergence blocker.
- **Was this external?** No. It is a local object-database packing state.
- **Could it recur?** Yes, for any repository that commits a large un-ignored
  file repeatedly before a repack. Polis is the demonstrated instance; the
  class is "large re-committed un-ignored session export lands loose". The
  daemon's existing auto-gc path triggers on *garbage* thresholds, not on
  loose-but-reachable bloat, so it does not self-heal this shape.

## Scope honesty

This verifies the guard refusal is resolved and the real hook now passes on the
current staged epoch. It does **not** establish that polis published, that the
three local commits ahead of `318182d1e` reached the remotes, or that the
fleet is clean and idle. Publication is the daemon's to finish; it was not
pushed manually.

## Raw evidence

| file | contents |
|---|---|
| `guard-pre-repack.json` | full guard JSON before (`code=BUCKET_STRATEGY_HIGH_WATER`) |
| `guard-post-repack.json` | full guard JSON after (`code=OK`) |
| `pre-repack-objects.txt` | `count-objects -v` + loose/pack byte split before |
| `post-repack-objects.txt` | same after |

## Addendum — 2026-10-29 15:40 UTC: `cargo deny` gate repaired

While starting the workspace gate for task 4.1, `cargo deny check` failed:

```
error[vulnerability]: libz-sys build script leaks environment variables in the build directory
    ├ ID: RUSTSEC-2026-0418
advisories FAILED, bans ok, licenses ok, sources ok
```

`libz-sys <= 1.1.30` prints its full build environment to stdout under `cc >=
1.6.0`, and cargo stores build-script stdout under `target/` — so any CI cache
or shared `target/` leaks those values. The workspace was pinned at 1.1.29.

Fixed by a real dependency upgrade, **not** an ignore entry:

```
cargo update -p libz-sys --precise 1.1.31
```

`deny.toml [advisories] ignore` remains `[]`. Re-run: `advisories ok, bans ok,
licenses ok, sources ok`.

Adding an advisory ignore here would have been the tempting shortcut and would
have left the leak in place for every future build-cache publication. The
patch release is the durable repair.
