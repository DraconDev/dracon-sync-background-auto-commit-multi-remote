## Review
- Correct: Live reclamation remains disabled by `LOSSLESS_RECLAIM_QUALIFIED = false` (`src/git/epoch_spool.rs:12`). This is not completion or drain authorization.
- Correct: Legacy directories without a producer-compatible lock are refused; inactivity additionally requires an identity-bound settled marker (`src/git/epoch_spool_ownership.rs:312–372`). Earlier unsafe removals are not qualification evidence.
- Correct: Detached recovery preparation owns the kernel fence; source hashing likewise receives ownership of that fence (`src/git/epoch_spool_recovery.rs:534–550`, `src/git/epoch_spool_source.rs:220–236`).
- Correct: Unique-payload verification uses a freshly initialized isolated ODB and checks enumeration plus object/type/size readback (`src/git/epoch_spool_unique.rs:77–158`).
- Correct: Final source checks precede deletion without another await (`src/git/epoch_spool_reclaim.rs:166–177`). Staged-epoch cleanup refuses reclamation when settlement fails (`src/sync/staged_epoch.rs:714–726`).

### Finding
**P1 — Metadata CAS silently substitutes another source’s permissions.**

Locations:
- `src/git/epoch_spool_cas.rs:173–209`
- `src/git/epoch_spool_recovery.rs:654–658, 771`
- `src/git/epoch_spool_source.rs:132–137`

Concrete counterexample:
1. Prepare a record containing metadata bytes X with mode `0644`.
2. Prepare another containing identical bytes X with mode `0600`.
3. `share_metadata` keys solely by content digest and accepts the existing CAS inode.
4. It replaces the second record’s copy with a hardlink to that `0644` inode.

The second record no longer preserves the source permissions. The explicit bytes-and-mode comparison ran **before** sharing; subsequent manifest verification hashes bytes only, so this substitution can still receive deletion authority once qualification is enabled.

**Minimum durable correction:** Include preserved permission bits in metadata CAS identity, validate those bits on reuse, and recheck archived metadata against the source snapshot after sharing. Add a two-record fixture with identical bytes and different modes, covering both insertion orders.

### Review limits
Read-only current-source review; no tests or live operations performed. `watchdog_diff` reported no Git-source working-tree delta against launch HEAD; no committed-range artifact was supplied.

### Merge verdict
**BLOCK qualified reclaim enablement** pending the metadata correction and renewed current-source qualification. Keeping the interlock false remains appropriate.

## BLOCKERS
- Correct metadata CAS permission substitution before enabling deletion.
- This review does not qualify any live drain or historical removal.