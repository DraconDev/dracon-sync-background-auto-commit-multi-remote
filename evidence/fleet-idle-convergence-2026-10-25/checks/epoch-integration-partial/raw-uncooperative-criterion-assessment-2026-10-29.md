# Why the raw-uncooperative negative can never pass (2026-10-29)

## The claim being tested

`qualify-exchange-syscalls.py` without `--warden` opens a **raw, uncooperative**
writer on the pre-exchange canonical inode, then lets the real helper perform its
single `RENAME_EXCHANGE`, and reports `pass` only when:

```python
record['pass'] = (a['newerWriterStillCanonical']
                  and a['canonicalContainsNewerWrite']
                  and a['indexPreserved']
                  and a['oldWriterLandedOnCanonical'])
```

Current receipt (`exchange-syscalls-raw-negative-current-2026-10-29.json`):

```
oldWriterLandedOnCanonical: false
oldWriterLandedDisplaced:   true
newerWriterStillCanonical:  true
canonicalContainsNewerWrite: true
indexPreserved:             true
pass: false
```

## Why `oldWriterLandedOnCanonical` cannot be true here

The writer's file descriptor is opened **before** the exchange
(`f=os.open(sys.argv[1], O_WRONLY)` while the helper is stopped at
`renameat2` entry). A file descriptor binds to an **inode**, not to a path.
`RENAME_EXCHANGE` atomically swaps two directory entries; it does not — and no
Linux interface can — retarget an already-open descriptor to a different inode.

So for a writer that opened before the exchange:

- the descriptor necessarily continues to reference the inode it opened;
- after the exchange that inode lives at the private/recovery path;
- therefore `oldWriterLandedOnCanonical` is necessarily **false**.

This is not a bug that more work can fix. It is arithmetic on descriptor
semantics. The criterion asks the kernel to do something the protocol cannot do,
which is exactly why the design labels these writers **unsupported** rather than
covered:

> Uncooperative/unknown writers remain unsupported, not a passing control.
> The current raw negative returns helper success while the old writer lands on
> the displaced inode; parent settlement/refusal for that exact timing must be
> qualified before anyone claims it blocks publication.
> — `docs/design/canonical-writer-fence-2026-10-27/PLAN.md`

## What the design actually promises for that writer — and does

The promise is not "its write reaches canonical". It is:

1. the newer cooperative/verified writer keeps canonical identity — **observed
   true** (`newerWriterStillCanonical`, `canonicalContainsNewerWrite`);
2. the index is untouched — **observed true** (`indexPreserved`);
3. bytes written to the displaced inode are **retained and surfaced as a real
   concern**, and **block publication** rather than being silently absorbed.

Promise 3 is covered by a real regression test that passes:

```
excluded_recovery_late_displaced_writer_after_archival_stays_visible_and_blocked
→ ok (1.26s)
```

That test reproduces precisely this shape (a writer that opened the displaced
inode, writes after archival with the same byte count **and a restored mtime**,
so only `ctime` exposes it), and asserts:

- `has_pending_excluded_recovery` becomes true — "archival is not immutability proof";
- `settle_pending` refuses;
- `sync_repo` returns `SyncOutcome::Blocked`;
- the seal is never rewritten to bless changed bytes;
- the index is unchanged.

## Consequence for the goal

`raw pass=false` is a **correct strict measurement of an unsupported boundary**,
not evidence of an unmet requirement. It must not be:

- flipped to `pass=true` (that would invert a deliberate negative control);
- deleted or relaxed (that would remove real evidence);
- used to claim uncooperative-writer safety.

It also must not block qualification of **unrelated repairs**. Nothing about the
loose-object auto-GC trigger or the decided-hook-verdict rule depends on the
canonical-writer exchange boundary; those changes touch `policy.rs`,
`git/mod.rs`, `sync.rs`'s publication fan-out and their tests, not
`excluded_revert.rs`.

What genuinely remains open from the fence work is the item the PLAN itself
lists as a qualification obligation — expanding the *cooperative* schedule to
more supported writers — not "make the uncooperative control pass".
