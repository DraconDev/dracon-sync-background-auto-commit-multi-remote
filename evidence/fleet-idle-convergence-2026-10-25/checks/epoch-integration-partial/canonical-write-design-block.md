## Review

- **Finding: P1 — queued-open rollback defect is now demonstrated.** `excluded_revert.rs:798–802` can move a newly opened canonical writer’s inode into private recovery storage. The supplied syscall receipt shows newer FD inode `36999145`, canonical inode `36999126` after rollback, and the newer write landing privately; index preservation does not satisfy worktree preservation. The qualification script pauses real syscall boundaries and forwards signals without modifying registers or source. This supersedes my earlier theoretical-only assessment.

### Candidate design: plausible direction, not yet qualified

Updating an existing inode **in place** avoids this particular FD-relocation failure. However, exclusive WRITE lease plus fatal SIGIO is not sufficient by itself.

Required constraints:

1. **Durable recovery before mutation.** Capture complete original bytes and metadata, desired filtered payload, repository/path/HEAD provenance and canonical inode identity. Verify the original under the acquired lease; fsync both payloads, manifest and containing directories before any write or truncate. A fingerprint alone cannot recover overwritten bytes. Preserve required filters and current policy/index-ownership checks.

2. **Fail closed on lease uncertainty.** Configure fatal, unblocked SIGIO and explicit signal ownership before acquiring leases, scoped to the dedicated helper. Account for inherited dispositions/masks, pending signals and helper threads. Close unrelated observation FDs before WRITE-lease acquisition. Held readers, writers, mappings, unsupported filesystems and permission failures must conservatively defer—not trigger chmod, lease bypass or fallback replacement.

3. **Do not claim kill implies atomicity.** SIGIO/SIGKILL, short writes, ENOSPC, truncate failure and lease expiry can leave partial canonical content. An in-flight syscall may already have modified bytes. Qualify actual ordering; successful signal configuration is not proof that no writes can overlap lease release or timeout.

4. **Never automatically restore old bytes over uncertain canonical content.** After interruption, retain before-image, desired payload and evidence; diagnose partial or newer canonical bytes without replaying either payload. Lease protection is inode-based, not a pathname lock: concurrent rename/unlink/replacement needs independent identity checks and conservative outcomes.

5. **Absent-path NOREPLACE may remain**, provided there is no subsequent rollback that displaces a newly acquired writer. Preserve finite cancellation and truthful mixed-transaction reporting.

### Settlement/archive changes are mandatory

Current `settle_directory` (`excluded_revert.rs:529–595`) classifies using the private payload alone. With in-place mutation, desired bytes remain private even after a partial canonical write: the existing `"untouched-or-rolled-back"` branch would falsely settle that transaction.

Use a **versioned, explicitly discriminated protocol**:
- Keep legacy exchange-record interpretation unchanged.
- New in-place records retain independently verifiable before/desired payloads.
- Separate “evidence safely archived” from “canonical reconciliation resolved.”
- Unknown/partial canonical outcomes remain persistent concerns, including after restart or policy opt-out.
- Extend archive seals to cover both payload sets and request identity; never reseal changed evidence as healthy (`excluded_archive.rs:162–239`).

### Important counterexample / material decision

For generated `.gitignore`, cancellation during in-place rewriting can expose truncated rules or missing protected negations once the helper dies. A durable backup does **not** prevent consumers from using that partial file. Thus this variant trades atomic visibility for inode continuity; it is not automatically semantics-preserving, particularly under the goal’s prohibition on weakening security guards.

Implementing and testing a new fail-closed journal protocol is within the repair scope. Accepting **backup-only preservation with potentially partial canonical metadata**, or relaxing existing visibility/security guarantees, requires an explicit material decision—not an implementation assumption. If those guarantees remain mandatory, keep that mutation blocked until a qualified protocol satisfies them; do not disable producer activity or silently exempt eligible work.

### Required qualification and verdict

Extend the deterministic syscall harness to lease acquisition/break, writes/truncate/fsync, cancellation, timeout, concurrent pathname replacement and restart settlement. Assert actual FD destinations, complete original retention, partial-byte diagnosis, unchanged index and non-clean reporting—not merely helper exit.

**Merge verdict: BLOCK.** The existing rollback is defective; the proposed replacement remains unqualified. No edits or execution performed. Evidence reviewed identifies helper SHA-256 `b7b98b2b7251858261a1e3227e654bda5e1b84b9aeeb25f114453ecb354a0d49`; no deployment or fleet acceptance is implied.