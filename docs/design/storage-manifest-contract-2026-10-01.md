# Portable restore manifest and sticky enrollment contract

Status: bounded private codec and retained Warden preparation implemented;
production publication and Git bridge remain gated. Date: 2026-10-01.

## Trust and protection

The standard pointer names immutable payload bytes. The restore manifest supplies
the owning repository identity and portable backend/security requirements. Neither
a pointer nor a manifest authorizes network access. An operator-controlled binding
must match the repository ID and approve each named backend before any access.
Repository content cannot introduce credentials, endpoints, local paths, commands,
recipients, public access or weaker security.

`storage_core::manifest` currently encodes **private plaintext**, for input to the
approved metadata-security adapter. It does not encrypt, install attributes,
stage a file or publish anything. Do not commit its `encode_private` output.
Sensitive metadata must be protected before it enters Git. The initial private
profile uses whole-manifest age encryption through Warden and preserves the
approved ciphertext across retries and unchanged staging, just like an asset's
prepared representation. The Git bridge must explicitly arrange this; assigning
an age filename or stacking filters is not protection.

The location of protected metadata must be bound during setup, excluded from
external asset matching, and checked for conflicting existing files/attributes.
No manifest pathname is installed by this codec. A future public metadata mode
requires its own explicit approval and exposure review; it is not a plaintext
fallback when keys or Warden are unavailable.

Age's authenticated decryption establishes ciphertext integrity. It does not
establish who authored repository metadata: an outsider with a public recipient
can produce encrypted content. Git trust, operator bindings and exact outgoing
reference validation remain necessary. Decode errors never authorize repair,
backend substitution or weaker protection.

## Version-one decoded format

The root has `version = 1`, a stable 64-lowercase-hex `repo_id`,
`retention = "preserve-all"`, and a sorted `enrollments` array. Each enrollment
contains:

- `path_hex`: lossless relative path bytes, using the journal's validation.
- `contract_sha256`: approved enrollment-contract identity, not a source digest.
- `primary` and `required_copies`: portable operator-defined backend IDs;
  copies are sorted, unique, nonempty, and include the primary.
- `encryption`: `warden-age` or an explicitly classified `none` representation.
- `payload`: the current representation's SHA-256 and length, or `null` for a
  deleted-path tombstone.

Encrypted payload fingerprints identify ciphertext. The manifest contains no
plaintext source fingerprints, URLs, credentials, executable configuration,
recipient lists or private keys. Non-sensitive unencrypted payload identities
still require the appropriate approved classification. Unknown fields, unknown
versions, duplicate JSON fields, duplicate paths, unsafe paths, malformed digests
and unsupported retention/security values fail closed.

Parsing is bounded to 4 MiB and 10,000 enrollments. Encoding uses a bounded writer
and stops at the same byte limit instead of allocating an oversized serialized
manifest. These are initial metadata-format limits, separate from large-payload
spool budgets. Larger collections require an explicit format/sharding decision;
they must not silently drop entries. The size bound also limits deeply nested or
oversized untrusted JSON input; serde's nesting limit remains enabled.

`matches_pointer` compares both payload digest and length. A tombstone cannot
match a pointer. Sorted paths and copy sets give deterministic private encoding.
Deterministic plaintext does not make fresh age encryption deterministic: the
approved encrypted manifest must be retained and reused.

## Sticky enrollment and version changes

An ordinary payload edit changes only its current payload identity. File size
shrinking below the original selection threshold does not change enrollment.
Deleting a file changes its current identity to a tombstone, preserving the
external-placement contract. Recreating the path therefore retains that contract.
Disabling new external enrollment does not prevent historical restoration.

`same_contract` compares path, contract identity, primary/copy requirements and
security independently of current bytes. It is a consistency check, not proof
of authorized enrollment. Production resolution must separately verify that the
contract digest was derived from an approved canonical contract. A backend,
security or enrollment-contract change requires explicit migration with verified
old-version recovery; normal staging cannot silently accept it. No migration API
or live sticky-enrollment installer is provided yet.

## Git and cold recovery gates

The Git bridge must build the complete manifest version from exact prepared jobs,
verify every required object copy, protect the metadata, and stage matching
pointers and protected metadata in one index transaction. It must inspect manual
index changes and preserve unrelated staged edits. Partial staging cannot advance
the job to committed/preserved. Group/manifest serialization and crash recovery
must be tested before daemon wiring; codec correctness alone does not satisfy
this gate.

A cold recovery starts with the protected manifest and pointer from the selected
Git version, operator-approved bindings, and separately backed-up historical
age identities. It needs no original private retry journal. Decrypt the bounded
manifest, check repo identity, match the exact pointer and security requirements,
then fetch and verify the representation before decryption/publication. Try only
approved listed copies. Missing keys/copies, corrupt content or disagreement
produce a failing result without overwriting a modified working file.

Key rotation must preserve the identities required by historical manifest and
asset ciphertext. Never put recovery private keys in the manifest or assume the
current machine identity can decrypt every historical version. Packaged restore,
historical-key drills, independent-copy failover, production metadata integration
and real Git atomic-staging tests remain outstanding.

## Retained protected metadata preparation (2026-10-02)

`storage_core::metadata::MetadataStore` now prepares a bounded manifest with the
operator-bound Warden adapter in a dedicated private namespace. Its record binds
the stable repo ID, private decoded-manifest fingerprint and approved metadata
policy digest. Policy derivation/authorization remains the trusted caller's
responsibility, including authorized-recipient changes. The root must be reserved
separately from asset journals, object stores and source checkouts; production
configuration resolution must validate those bindings and root separation.

This record has capture/preparation states, not fabricated object destinations
or Git receipts. The plaintext is captured privately and verified before Warden
reads it. Bounded ciphertext output, protocol checks and successful child exit
precede fsync and durable approval. Publication/recovery verifies the approved
fingerprint before adopting the ciphertext. A complete unapproved spool cannot
become an approved candidate. Source snapshots and approved ciphertext remain
retained; only unapproved transform output can be reset after source verification.

Unchanged manifest/policy versions reuse the exact ciphertext without invoking
Warden or rewriting the prepared record. Approved missing/corrupt bytes fail
closed rather than selecting new randomness. Transient deadlines kill/reap the
child and retain a 30-second retry deadline. Permanent failures need explicit
intervention; clearing a failure is not evidence of repaired content.

The store uses existing private-file checks, managed runtime ignore protection,
atomic JSON replacement, process-backed leases and resumable source capture.
Caller-provided record/source/payload budgets apply, with additional format caps
of 4 MiB decoded and 8 MiB protected metadata. No eviction or working-file
deletion is implemented. A failed budget does not authorize an unprotected
manifest or raw asset fallback.

`open_prepared` provides a verified ciphertext file for the future Git transaction.
It does not stage anything or mark metadata committed/preserved. The actual Git
blob OID is computed by Git; the prepared SHA-256 checks the blob's content and
must not be substituted for Git's object ID.

Synthetic tests cover publication, reuse, foreign bindings, process death on
both sides of approval/publication, corrupt approved output, resource exhaustion
and retry deadlines. A separately run real Warden fixture encrypted a manifest
with a non-UTF-8 asset path, saved the ciphertext, removed only its test-owned
temporary metadata store and restored the exact decoded manifest using separately
retained fixture keys. No live keys, filters, repositories or buckets were changed.

## Atomic index publication infrastructure (2026-10-02)

`StageBundle` couples verified leased jobs to the exact manifest from which
retained ciphertext was prepared. Repository identity, sticky contract, security,
copy set and current payload must agree. Source and prepared payload snapshots
are reverified. Failed jobs, duplicate paths and unmatched metadata are refused.

`IndexTransaction` additionally requires the checkout's local
`dracon.storageRepoId` to match its approved binding. It writes ciphertext and
canonical pointers as Git blobs without filters, then prepares a complete index
candidate containing both. Unrelated staged entries survive. The actual index
fingerprint must still equal the observed baseline under Git's index lock.
Existing raw tracked assets require reviewed migration; staged managed edits or
deletions, unmerged entries and path/gitlink collisions block the transaction.

A private durable intent precedes publication. A complete candidate in the
repository Git directory is hardlinked to `index.lock`, then renamed over the
index atomically. Recovery recognizes its own lock by recorded device/inode and
never removes a foreign lock. A conflicting baseline releases only transaction
artifacts and leaves the operator index intact. Verification reloads the actual
index from disk rather than trusting libgit2's cache; only then may jobs become
`Staged`. Git commit and push receipts remain separate.

Prior encrypted metadata must resolve to a retained verified preparation before
its sticky enrollments can be changed. Entries cannot disappear or change
contracts implicitly, and new payload identities require verified jobs.
Cold-checkout decryption/import and reviewed enrollment migration are still
required production work. This local proof mechanism is not a complete restore
workflow or authorization source.

Tests use synthetic Warden subprocess output solely for index mechanics and
cover matched publication, unrelated staging, manual edits/deletions, foreign
locks, conflicting saved intents, non-UTF-8 paths, tombstones and process death
after intent publication, before index replacement and after replacement.
This library performs no working-tree writes, filter installation, commits,
pushes or automatic daemon enrollment. Working-file race checks and outgoing
commit validation remain required integration gates.
