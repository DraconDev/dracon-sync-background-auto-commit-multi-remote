# Portable restore manifest and sticky enrollment contract

Status: bounded private codec implemented; protected publication and Git bridge
remain gated. Date: 2026-10-01.

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
historical-key drills, independent-copy failover, protected metadata preparation
and real Git atomic-staging tests remain outstanding.
