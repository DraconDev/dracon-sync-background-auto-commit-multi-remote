# Exact-version storage journal contract

Status: shared-library implementation; no production transfer worker or enrollment.

## Identity and trust

A job binds the operator-bound repository ID, lossless relative path, selected
source SHA-256/length, effective policy digest, representation security class,
primary binding, required object copies and required Git targets. Its ID is a
domain-separated digest of that immutable specification. A changed source or
contract creates a different job; it cannot replace an existing job's version.
Raw source hashes and path identities stay in private local state.

These structures validate evidence consistency. They do not perform secret
classification, authorize a backend, prove recipient approval, or inspect the
Git index. Callers must establish those facts before recording receipts. The
Warden streaming adapter and copy executor exist separately; binding recipient
authorization and completed encryption artifacts to the executor remains
required before production use.

## Durable progress

```text
pending-capture -> captured -> prepared -> uploading
  -> primary-verified -> ready-to-stage -> staged -> committed -> preserved
```

The primary-verified phase is skipped when all required copies are already
verified. Readback receipts must match the exact prepared representation.
Staging requires every configured object copy; preservation additionally
requires acknowledgments of the recorded commit from every configured Git
target. These are historical receipts, not a live backend health guarantee.
The worker must reconcile actual external state after a crash or outage.

Each job has an exclusive OS lease. Atomic JSON replacement uses a private
spool, file fsync, rename and directory fsync. Revision checks reject stale
updates; transitions cannot discard immutable proof or skip required phases.
An OS lease is released on process death. No transfer needs a Git index lock.

Transient failures retain progress and respect their retry deadline. Credential,
capacity, integrity, security and source-change failures require intervention.
Clearing a failure is an explicit API operation, not proof the problem vanished.
Cancellation before publication preserves the record and captured bytes.

## Approved representation and copy execution

The job records the caller-approved ciphertext fingerprint before retaining its
bytes. This candidate is immutable and is not yet a durability receipt. The
caller must preserve its successfully encrypted input artifact until capture
completes. A full verified payload spool can be recovered from its recorded
identity without repeating encryption. Partial payload capture resumes only
from the same matching representation; new ciphertext randomness cannot replace
an already selected candidate. Payload publication precedes the prepared phase.

The copy executor reads only the retained prepared payload and rechecks every
required destination on each run, including those with old receipts. It saves
the upload attempt before I/O and each successful readback before advancing.
An apparent upload success without readback cannot create a receipt. Structured
integrity/capacity failures block progress and retain bytes. Transient I/O
failures keep proof and establish a 30-second retry deadline. The daemon's fair
scheduler and provider-specific classification remain pending.

## Source capture and exhaustion

Capture streams through a 64 KiB buffer into a private version-specific spool.
A partial spool can resume only if the supplied input matches its existing
prefix; final byte count and digest must match the selected source. A completed
snapshot is reused after verification even when the live file has since changed.
Complete spools left by process death are verified and adopted before reading
new input. Corrupt or conflicting snapshots are retained and refused.

The current library limits are per repository: 10,000 job records, 1 GiB per
source version, 4 GiB aggregate source/capture bytes, 2 GiB per prepared payload and 8 GiB
aggregate payload/spool bytes. A local capture-budget
lease prevents simultaneous captures from exceeding the aggregate limit.
Exhaustion blocks capture without deleting retained data. Source and prepared-payload budgets are separate; cache, global disk reserve
and operator policy limits still need integration.

The Unix adapter requires private operator-owned directories/files and rejects
symlink components and hard-linked journal entries. The journal contains
plaintext source snapshots; it must remain private and must never be committed
or sent to an object backend as the encrypted representation.

## Inspection and tested recovery

`storage status` reads validated records without creating paths, acquiring job
leases, running filters, or accessing a backend. It omits paths and hashes.
Invalid or failed records yield a nonzero result and remain unchanged. It does
not repair corrupted records, delete old versions, or start a transfer.

Tests exercise exclusive leases, stale revisions, forbidden regressions,
copy/Git acknowledgment gates, typed retry backoff, corrupt records, byte
budgets, partial captures, changed inputs and process death on both sides of
record and snapshot publication. Production worker/index/network reconciliation, security subprocess
composition and independent recovery drills remain
release gates in the [delivery ledger](storage-delivery-ledger-2026-10-01.md).
