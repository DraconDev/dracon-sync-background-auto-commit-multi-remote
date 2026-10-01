# Storage representation decision: standard pointers, explicit security composition

Status: encoding selected for implementation; production enrollment remains gated.
Date: 2026-10-01.

## Decision

Use the standard Git LFS v1 pointer encoding for payload identity instead of
inventing another pointer syntax. For encrypted assets, the OID is the SHA-256
of the encrypted representation and `size` is its byte length. A versioned Git
manifest provides the additional approved backend binding, stable repo identity,
security/restore metadata, and enrollment contract. Protect manifest metadata
with Warden where required and commit it atomically with matching pointers.

The Git bridge and durable transfer scheduler still belong to Dracon Sync.
Git selects one effective filter, so the external-payload filter must explicitly
compose the required security adapter. Stock LFS and Warden attributes cannot
simply be stacked. A prepared snapshot produces a deterministic local clean
result; a changed source without matching preparation must fail. Never upload
or download from clean/status/diff. Explicit hydration handles network restore.

This selects an interoperable encoding, not a claim that the first release is
a drop-in stock Git LFS workflow or an LFS server. S3-compatible/local adapters
resolve approved bindings through Sync. Git LFS is not required on every target
installation just to parse a pointer or recover through the packaged CLI.
Existing LFS-managed paths retain their existing workflow until an explicitly
reviewed compatibility/migration path is tested. Conventional LFS-server
interoperability is a separate adapter/conformance obligation if offered.

References:
- https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md
- https://github.com/git-lfs/git-lfs/blob/main/docs/custom-transfers.md
- https://git-scm.com/docs/gitattributes

## Evidence

`python3 scripts/storage-representation-prototype.py` ran successfully with
Git LFS 3.7.1 and isolated age keys. It used only temporary repos, disabled
hooks/global configuration, and no network transfers or live buckets.

- An age ciphertext payload produced the exact standard three-line LFS pointer.
- The local LFS object restored/decrypted to the exact original plaintext.
- Assigning two filter attributes selected the last driver, rather than
  composing Warden and LFS.
- A prepared-reference clean filter kept usable plaintext in the working tree.
- A source edit caused required-clean failure without mutating the existing
  index or substituting a stale reference.
- A cold local clone contained the portable reference; verified encrypted bytes
  decrypted to the exact original payload without original local job state.

The comparison's custom reference was fixture-only JSON. It is not the selected
production pointer format. Its preparation/race experiment establishes a useful
Git interaction pattern without requiring a second pointer encoding.

The shared Rust local backend separately passed create-only publication,
readback digest/length checks, corruption refusal, interrupted capture, and a
101 MiB cold-reopen round trip using 64 KiB read buffers. Its explicit operational
age test also passed with isolated fixture keys.

## Security boundaries and incomplete gates

The experiments prove opaque encrypted-byte recovery. Subsequent isolated
Warden CLI tests also prove bounded whole-payload age encryption/decryption
through existing authorized recipients and identity discovery, including an
untrusted repo recipient refusal and a 101 MiB exact round trip. Production
classification, metadata confidentiality, and large-payload Git filter/worker
composition remain mandatory before enrollment. Do not interpret the pointer encoding
choice as permission to weaken the Warden adapter or lift its current bounds.

Plaintext hashes used to match prepared source versions stay in protected local
state. Public pointers for encrypted content carry ciphertext identity, not
plaintext content identity. Repeated clean operations reuse the same prepared
ciphertext/reference; age randomness cannot make unchanged files appear changed.

Git tree filenames already expose tracked paths; this feature must not create
additional plaintext path/digest metadata or bucket keys outside that existing
exposure. Sensitive manifests and restore information require appropriate
protection. Backend identifiers resolve through operator-approved configuration;
references do not authorize arbitrary endpoints, commands, or credentials.

Before implementing production pointers/manifests, specify and test schema
versioning, atomic staging, historical key recovery, sticky enrollment, reference
validation, and missing-object exit behavior. Live provider capability checks,
independent recovery-copy drills, and packaged-install tests remain release gates.
