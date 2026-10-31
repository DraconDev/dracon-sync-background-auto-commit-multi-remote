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

## Proposed recovery transaction (under adversarial review)

1. Under ownership, validate spool identity and enumerate all regular metadata and pack inputs. Unknown subdirectories, split-index dependencies, symlinks or incomplete enumeration must fail closed until explicitly supported.
2. Create a private unique working directory under the Git recovery area, not predictable shared `/tmp` names. Rebuild derived pack indexes from stable pack bytes. Parse aligned output with whitespace-aware fields and validate object format, per-pack counts and complete enumeration. Deduplicate OIDs across packs without mistaking overlapping packs for corruption.
3. Preserve exact index and metadata in a versioned recovery record. Enumerate required OIDs from all retained packs and index stages; retain submodule gitlink identity as metadata, not as an obligation to supply another repository's commit locally.
4. Compact absent objects into an indexed standalone rescue pack using file-based plumbing rather than unbounded binary stdout capture. Verify every rescued OID, type, size and content hash in an isolated store.
5. Protect objects already in the main ODB from ordinary GC as well. Candidate mechanism: a synthetic recovery tree enumerating blob/tree OIDs plus a recovery commit whose parents retain commit OIDs, bound to a dedicated recovery ref. This mechanism is NOT adopted until review confirms it cannot be exported accidentally by mirror pushes or violate the no-published-history boundary. Resolve linked-worktree common-directory behavior and closure validation before use. If ref anchors are unsuitable, use an independent durable recovery store with equivalent retention guarantees.
6. Durably install versioned metadata, object manifest, rescue artifacts and a receipt binding the installed retention authority. Use create-only/atomic identities, fsync files and containing directories, and never overwrite an existing valid rescue. Receipt failure preserves the original.
7. Recheck the fenced spool identity and preservation markers, then remove only the validated original. Failures must not delete rescue artifacts or weaken evidence. Record net savings including metadata, rescue and retention-store cost.

## Required qualification

Real-process fixtures must demonstrate old-but-live owner exclusion, owner death and safe release, settled-owner reclaim without self-lock, concurrent reapers, and legacy refusal. Real Git fixtures must demonstrate index/metadata reconstruction, mixed rescue, corruption after pack write preserving the original, unsupported index preservation, ordinary GC survival and failure at each durable-install boundary.

Non-vacuity must be rerun against the finished source in disposable copies. The original prototype's mutation record qualifies only that historical predicate, not this redesign. Workspace tests, strict Clippy, release build and deny must all pass; incomplete or timed-out gates remain failures/incomplete evidence.

## Live execution and accounting

Only after qualification, capture a fresh consistently scoped fleet inventory. Select the largest currently eligible mixed spool for a live rescue proof. Do not reuse the disappeared `ai-auto-writer/dracon-staging-2ag7qb` as executed proof: its disappearance remains unexplained. Historical drain logs must be retained with their safety limitations.

Report protected and uncertain survivors rather than deleting them to satisfy a count. The corrected no-removable-duplicates check applies only to provably inactive eligible spools. Include rescue-store growth in net reclamation accounting; relocating bytes is not freeing space.
