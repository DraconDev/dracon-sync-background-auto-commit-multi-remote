# Epoch spool durable recovery — approved safety redesign

Status: design under implementation, not qualified. Destructive reclaim remains disabled by `LOSSLESS_RECLAIM_QUALIFIED = false`. The earlier presence-only prototype is not an accepted implementation.

## Intent and corrected invariants

The revised operator-confirmed objective requires recovery objects AND index/metadata to survive before a spool is removed. Readability in the main ODB at one instant is not durability: dangling objects can already be past prune grace, and inherited alternates do not establish retention. Neither an old directory nor a newly manufactured lease proves a legacy producer has finished.

The absolute requirements are:

- Retain exact recovery semantics: original index bytes and the files/metadata needed to reconstruct paths, modes and stages.
- Maintain durable access to the complete required object closure through ordinary Git GC; do not rely on ordinary prune grace.
- Prove an independently installed rescue by full OID/content reads in an isolated object database with no main/spool/alternate fallback.
- Acquire authoritative cross-process ownership before examining mutable spool state and keep it across every await and final removal. An age floor only schedules work.
- Preserve `preserve-rejected`, `original-index`, active/fresh, legacy-uncertain, unsupported and failed-proof spools.
- Persist artifacts and a recovery receipt before deleting the original. Every crash before that point must leave the original, a complete recovery record, or both.
- Do not alter user branches or published history, delete rescue artifacts, stop/pause the daemon, or broaden age-only apply authority.

## Lifecycle ownership

`Staging::new` now creates and locks `reclaim-owner.lock`; `Staging` owns the FD until it transfers the same `SpoolLease` to `Epoch`. The sweeper opens an existing lock without creating one and skips lockless legacy spools. This is preliminary fencing, not final qualification.

Settled deadline cleanup must use an explicit ownership handoff or owner-authorized API; opening the same lock independently while the epoch retains its FD self-contends. Child termination must precede handoff. The final protocol must also bind lock inode and spool identity, reject symlinks and unsupported payloads, and withstand two concurrent reapers and a process stalled past the age floor. Creation before lease acquisition may not admit a producer that lost ownership to a reaper.

## Recovery storage decision

The read-only falsification review rejected relying on a main-repository ref CAS: an already-running GC may have snapshotted refs before the new anchor, and a mirror push can export ordinary recovery refs. The selected direction is an independent recovery object store under `dracon-rescue`, with no main-repository recovery refs or remotes. Its installed objects and complete recovery closure must be readable without the original spool, main ODB or alternates. Content-addressed storage is shared across recovery records to avoid repeatedly retaining the same baseline.

Use stable source snapshots only to build the recovery transaction. Persist a complete closure independently before removing the original; protect installed recovery packs against GC in that independent store. Isolated verification must hash complete content, not merely ask `cat-file -t`. Report the independent store's actual cost alongside spool savings; do not claim moving or pinning bytes is net reclamation. The exact storage transaction still requires implementation and qualification.

## Recovery transaction (under implementation)

1. Under ownership, validate spool identity and enumerate all regular metadata and pack inputs. Unknown subdirectories, split-index dependencies, symlinks or incomplete enumeration must fail closed until explicitly supported.
2. Create a private unique working directory under the Git recovery area, not predictable shared `/tmp` names. Rebuild derived pack indexes from stable pack bytes. Parse aligned output with whitespace-aware fields and validate object format, per-pack counts and complete enumeration. Deduplicate OIDs across packs without mistaking overlapping packs for corruption.
3. Preserve exact index and metadata in a versioned recovery record. Enumerate required OIDs from all retained packs and index stages; retain submodule gitlink identity as metadata, not as an obligation to supply another repository's commit locally.
4. Compact absent objects into an indexed standalone rescue pack using file-based plumbing rather than unbounded binary stdout capture. Verify every rescued OID, type, size and content hash in an isolated store.
5. Protect objects already in the main ODB as well: install every needed object in the independent recovery store, verify complete tree/commit/tag closure there, and make preservation durable before deleting the source. A transient main-object lookup or a new main-repository ref cannot supply this proof. Resolve the originating worktree gitdir separately from the main common object directory. Avoid unbounded parent argv and binary stdout capture.
6. Durably install versioned metadata, object manifest, rescue artifacts and a receipt binding the installed retention authority. Use create-only/atomic identities, fsync files and containing directories, and never overwrite an existing valid rescue. Receipt failure preserves the original.
7. Recheck the fenced spool identity and preservation markers, then remove only the validated original. Failures must not delete rescue artifacts or weaken evidence. Record net savings including metadata, rescue and retention-store cost.

## Ownership checkpoint

`cargo test --locked -p dracon-sync --bin dracon-sync epoch_spool::ownership` passed five tests after extracting the ownership protocol into `src/git/epoch_spool_ownership.rs`. These demonstrate missing legacy metadata refusal, a real child holding ownership despite old mtime, lock release on child exit, explicit settled-owner handoff and competing-reclaimer exclusion, and replaced-lock/symlink/outside-gitdir rejection. They qualify only that boundary, not destructive reclamation. Descendant lifetime during process death/cancellation remains unresolved: CLOEXEC means a child does not automatically hold its producer's lock.

## Recovery-engine checkpoint

`epoch_spool_recovery.rs` now builds independent template-free bare recovery records, preserving exact supported index/metadata bytes and complete pack/index closure for SHA-1 and SHA-256. It refuses unsupported index dependencies. The disconnected engine returns a receipt and never removes its source. The first component stored a complete closure pack per record. The subsequent shared-object implementation below removes that repeated bulk; no live net-savings claim is justified.

Independent source review found an unjoined-child cleanup race and missing prepublication fault coverage. Follow-up changes give preparation its own owned task, join killed Git plumbing before releasing temporary storage, bound captured output, and impose a caller-side transaction deadline. Cancellation may leave preparation running or a retained record, but cannot authorize source deletion. These controlled bare Git plumbing commands are not general hook/filter execution; the separate producer lifecycle still needs containment.

`cargo test --locked -p dracon-sync --bin dracon-sync epoch_spool::recovery` passed nine tests after these corrections, including real child cancellation retaining temporary storage, post-write corruption before verification, failure before receipt publication, index reconstruction without the original store, overlapping packs and tag/commit closure, SHA-1/SHA-256, and ordinary GC survival. Output: `docs/evidence/epoch-spool-reclaim-2026-10-31/recovery-review-fixes.txt`. This is component evidence, not full workspace, integrated deletion or crash/power-loss qualification.

## Shared-storage checkpoint

`epoch_spool_cas.rs` now unpacks complete closure packs into a fresh isolated, format-matched bare builder, validates complete enumeration, and installs create-only loose-object hardlinks under `dracon-rescue/cas/objects`. Each record gets independent hardlinks and private recovery refs, not alternates or main-repository refs. Existing CAS objects are never overwritten or repaired; reused content is checked by final isolated fsck/readback before publication. Shared object inodes, fanout directories and ancestors are synced before a receipt. Only unpublished transaction-owned packs are moved into temporary work storage.

Version 2 receipts bind immutable source/index metadata and semantic object/root inventories, rather than physical ODB filenames that ordinary record GC can change. `verify_receipt` checks the published receipt, immutable file hashes, required dependencies, isolated object contents/closure, types and sizes. A real-Git fixture checks physical inode sharing, record GC survival and metadata-corruption rejection. Another proves an existing corrupt CAS object is neither accepted nor overwritten. The first corruption-fixture attempt hit Git's read-only object mode; the deliberate fault now changes permissions only inside its disposable fixture before corrupting bytes.

Preparation also has an ownership-preserving API: its detached task owns the actual reclaimer kernel fence and returns it with the receipt. The cancellation fixture now proves that another reclaimer cannot acquire that fence while the cancelled request's writer remains alive; it becomes available only after process join and cleanup.

Current component checks: recovery suite 11 passed, zero failed; producer suite 4 passed, zero failed, zero ignored when explicitly including the host test. Output is in `recovery-shared-tests.txt` and `producer-qualified-tests.txt`; matching helper build output is `producer-helper-build.txt`. These are not integrated-reaper or workspace qualification. Still required: compact unique-payload rescue-pack sidecar/readback, source fingerprint/final checks, identity-bound producer-settled marker, staging/epoch lifetime integration, guarded shared reclaim and final fault/non-vacuity/full-gate/live evidence.

## Producer-settlement decision

A kernel lock released by a cancelled producer is not a descendant-completion proof. The inspected cancellation helpers currently signal groups without joining a whole family. A disposable host probe confirmed that a transient user service with `ExitType=cgroup` remains populated after its leader exits while a `setsid` descendant with closed stdio is alive. Select narrow epoch-only containment, tracked outside request cancellation, plus verified cgroup emptiness before a durable identity-bound settled marker. This is an ordinary descendant-lifetime boundary, not protection against deliberately hostile same-UID cgroup escape. No daemon stop/pause or configuration change is authorized by this decision.

`epoch_spool_producer.rs` now contains a disconnected explicit-command scope, pre-Tokio exact-environment exec trampoline, retained launch tasks, cgroup-bound invocation checks and bounded settlement. Four focused tests passed including the explicitly selected host fixture, against a freshly built helper: cancellation before polling a registered launch, closed-stdio `setsid` descendants, numeric exit 1 and exact literal/removed-variable forwarding. The host fixture also proves a source-mismatched trampoline rejects its target before execution. A first host run failed because a completed failed service had already removed its cgroup; the corrected path accepts disappearance only after the start is resolved, the invocation is bound and manager state is terminal. Fast command launches bind the expected unit path from the manager's app slice before start. Settlement clears only its own stopped transient unit's failed-state bookkeeping, not journal evidence.

Evidence is `producer-qualified-tests.txt` and `producer-helper-build.txt` in the recovery evidence directory; the initial failed host run is retained in `producer-host-initial-failure.txt`. This establishes component behavior only. Staging/epoch integration, source-bound settled markers, stop-timeout/fault coverage and final independent review remain unfinished; destructive reclaim is still disabled.

## Required qualification

Real-process fixtures must demonstrate old-but-live owner exclusion, owner death and safe release, settled-owner reclaim without self-lock, concurrent reapers, and legacy refusal. Real Git fixtures must demonstrate index/metadata reconstruction, mixed rescue, corruption after pack write preserving the original, unsupported index preservation, ordinary GC survival and failure at each durable-install boundary.

Non-vacuity must be rerun against the finished source in disposable copies. The original prototype's mutation record qualifies only that historical predicate, not this redesign. Workspace tests, strict Clippy, release build and deny must all pass; incomplete or timed-out gates remain failures/incomplete evidence.

## Live execution and accounting

Only after qualification, capture a fresh consistently scoped fleet inventory. Select the largest currently eligible mixed spool for a live rescue proof. Do not reuse the disappeared `ai-auto-writer/dracon-staging-2ag7qb` as executed proof: its disappearance remains unexplained. Historical drain logs must be retained with their safety limitations.

Report protected and uncertain survivors rather than deleting them to satisfy a count. The corrected no-removable-duplicates check applies only to provably inactive eligible spools. Include rescue-store growth in net reclamation accounting; relocating bytes is not freeing space.
