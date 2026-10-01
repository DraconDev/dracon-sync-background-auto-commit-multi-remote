# External-storage planning preview

This is an unreleased source-build preview. The installed 0.113.92 daemon does
not automatically move files into a bucket. The new commands are read-only;
`storage.enabled` currently enables rule simulation, not transfers. No command
in this preview stages files, installs filters, migrates history, or uploads.

From a source checkout, build `cargo build --locked`, then run the built binary:

```sh
dracon-sync storage validate --repo /path/to/repo --policy /path/to/operator.toml
dracon-sync storage plan --repo /path/to/repo --policy /path/to/operator.toml --json
dracon-sync storage plan --repo /path/to/repo --history --json
dracon-sync storage status --repo /path/to/repo --json
```

`--policy` selects an operator policy without changing it. When omitted, the
existing policy is used if present; an installation without one uses defaults.
The command works without a daemon. Repo paths resolve to their Git root.
Planning reads metadata and Git attributes without running clean/smudge filters.
It includes tracked files and unignored untracked files. It reports symlinks,
missing files, nested repos, existing filters, policy exclusions, and oversized
Git candidates rather than following links or treating them as saved assets.
Ownership, secret classification, enrollment, and security integration remain
separate required gates; proposed placement is not permission to upload.

The normal report totals current regular-file payload bytes by proposed
placement. These are not predicted pack sizes, Git history savings, or storage
billing estimates. `--history` additionally scans all local refs for unique
reachable raw blob bytes and reports the own Git object database disk bytes.
The database includes unreachable objects and excludes nested gitdirs. Neither
number is the next push's pack size. This explicit scan can be slow.

## Operator policy example

Add the following to an operator policy for simulation. Backend paths do not
need to exist: planning never opens or creates them.

```toml
[storage]
enabled = true

[storage.backends.archive]
type = "local"
root = "/absolute/operator-selected/archive"
# Default permitted class is warden-encrypted.
allowed_security = ["warden-encrypted"]

[[storage.rules]]
paths = ["assets/keep-in-git.png"]
placement = "git"

[[storage.rules]]
paths = ["assets/**"]
placement = "external"
min_bytes = 20971520
backend = "archive"
security = "warden-encrypted"

[[storage.rules]]
paths = ["renders/intermediate/**"]
placement = "external"
backend = "archive"
security = "warden-encrypted"
```

20 MiB is an example condition, not a global default. Rules are ordered: the
first path and size condition that matches wins. The minimum is inclusive.
`*` does not cross directory separators; `**` can. No match retains Git
placement and its existing safeguards. Invalid and duplicate identical
conditions are rejected. A matched Git rule does not grant an exception to
secret scanning or staging limits.

For an S3-compatible binding, the prototype schema accepts `type = "s3"`,
`endpoint`, `bucket`, `credential_ref`, and `allowed_security`. Validation
requires HTTPS without embedded credentials/query/fragment. Credentials and
buckets are not accessed. S3 transfer/capability checks are not implemented yet.
A non-sensitive rule is permitted only when the operator explicitly includes
`"non-sensitive"` in that backend's `allowed_security`. This is not a public
publication capability or proof that the content is non-sensitive.

## Repo overrides

`.dracon/dracon-sync.toml` can turn simulation off:

```toml
[storage]
enabled = false
```

A repo's `storage.rules` list replaces the global rule list completely. An
empty list removes inherited rules. Omitted values inherit. Repo settings
cannot define backend endpoints, credentials, or permitted security classes;
they select operator-approved bindings. Malformed storage settings and config
symlinks fail planning explicitly. Ordinary storage-disabled configs retain
the existing sync behavior.

The schema currently has no retention, publication, migration, or atomic-group
settings. Those roadmap fields are proposals and are rejected if supplied to
this preview. Enrollment persistence, retries, hydration, and production filter
composition are pending implementation.

## Local journal evidence

`storage status` reports validated local job records and their recorded phases,
failures, pending bytes, and completed preservation receipts. It performs no
network requests, starts no transfers, and creates no journal paths. A recorded
receipt is historical evidence, not a current probe of backend availability.
Malformed records and failed jobs produce a concern report and nonzero exit.

The repo ID comes from local Git config `dracon.storageRepoId`, or explicit
`--repo-id` for inspection. Missing enrollment reports an uninitialized journal.
`--state-dir` selects a state base; records live below its `storage-journal`
directory. Otherwise the command uses `DRACON_SYNC_STATE_DIR` or
`~/.dracon/utilities/sync`. Paths and payload hashes are omitted from status.

The shared-library journal has private per-job leases, atomic durable records,
exact-version snapshots and typed retry gates. Its current per-repo defaults
are 10,000 records, 1 GiB per source snapshot and 4 GiB retained source/capture
bytes, plus 2 GiB per prepared payload and 8 GiB retained payload/spool bytes.
These library defaults are not an enrolled operator storage policy.
Prepared candidates bind approved representation identities; complete payloads
are retained and reused across retries. The shared copy executor verifies every
required copy using operator-resolved adapters. Preview commands and the daemon
do not invoke that executor. Exhaustion refuses capture and retains bytes; no
automatic eviction exists. Unix ownership/permission checks are required by this adapter.

## Implementation checks

The immutable local backend is available in the shared library; the daemon and
preview commands do not call it to preserve files automatically. Its tests
exercise bounded streaming above 100 MiB,
create-only publication, readback verification, cold reopen, corruption,
interrupted capture, and object symlink refusal. The encrypted operational
check requires `age` and `age-keygen`:

```sh
cargo test -p dracon-sync encrypted_payload_round_trip --locked -- --ignored
python3 scripts/storage-representation-prototype.py
```

The Python prototype additionally requires Git LFS. It uses isolated temporary
repositories, independent fixture keys, disabled hooks, and no operator global
Git configuration. It compares a standard LFS ciphertext pointer with a local
prepared-reference filter; both restore exact plaintext bytes. It proves stale
source preparation is refused and a cold clone retains the reference. It does
not prove production filter composition, provider guarantees, or real
bucket recovery. No production pointer format has been enrolled.

The Warden source-build streaming adapter is separate from these experiments:

```sh
dracon-warden storage-encrypt --repo /path/to/repo --max-bytes 4294967296 < input > private-ciphertext-spool
dracon-warden storage-decrypt --repo /path/to/repo --max-bytes 4294967296 < private-ciphertext-spool > private-plaintext-spool
```

Use private destinations, and publish them only after exit status zero. Streamed
output can be partial on failure, including a damaged final authentication tag.
These commands use existing authorized recipients/keys; they do not enroll a
repo, install Git filters, generate keys, or upload. Classification and Sync's
production subprocess/manifest composition remain pending.
