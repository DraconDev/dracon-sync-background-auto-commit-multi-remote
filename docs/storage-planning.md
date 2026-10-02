# External-storage planning preview

This is an unreleased source-build preview. The installed 0.113.92 daemon does
not automatically move files into a bucket. Planning and status commands are
read-only; `storage.enabled` currently enables rule simulation, not transfers.
The explicit prepared clean driver described below writes a verified pointer
to stdout. No preview command stages files, installs filters, migrates history,
or uploads.

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
Reserved journal and writable local-backend directories receive managed private
ignore protection. Previously tracked runtime paths and project Git roots are
refused; unmarked nonempty directories are refused to protect existing operator
files from a blanket ignore. Modified protection blocks writes. This prevents private captures from
being committed when state lives inside a watched repository. Read-only commands
do not create or change those ignore files.

The [restore manifest codec](design/storage-manifest-contract-2026-10-01.md)
produces private plaintext for an approved metadata-security transaction. The
shared metadata store now retains Warden-encrypted Git-blob candidates with
crash recovery and unchanged-version reuse. It does not publish metadata,
install filters or authorize backend access; production configuration and Git
transactions remain pending.

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

The shared-library Warden preparation adapter now connects captured source bytes
to bounded subprocess output, durable approval and payload publication. It checks
exit status before approval, enforces a deadline and reuses approved saved output
after process death. No CLI/daemon storage enrollment invokes it yet. An explicit
operational test passes a 101 MiB fixture through real Warden, both local copies,
and cold decryption with isolated keys. To reproduce from the parent workspace:

```sh
DRACON_STORAGE_TEST_WARDEN=/absolute/source-build/dracon-warden cargo test -p dracon-sync --lib real_warden_large_payload --locked -- --ignored
```

The test also requires `age-keygen`. Its keys, source snapshots and backend roots
are temporary fixtures. Test copies share a physical filesystem and are not
certified independent recovery storage.

## Repository-wide prepared clean driver

`storage filter-clean` is an unreleased local driver for already prepared
versions, not an enrollment or upload command. Operator arguments bind an exact
repo, existing private state roots and reserved metadata path. The local Git
config `dracon.storageRepoId` must match. The actual Git index selects the
protected manifest; the driver resolves that exact ciphertext to its verified
local preparation, then selects the requested path's matching eligible job.
There are no per-version `--metadata-id` or `--job-id` arguments to update.

```sh
dracon-sync storage filter-clean \
  --repo /path/to/repo --repo-id "$REPO_ID" \
  --journal-root /private/storage-journal \
  --metadata-root /private/storage-metadata \
  --manifest-path .dracon/assets.manifest \
  -- 'assets/example.bin' < 'assets/example.bin'
```

One required Git driver can use `%f` after `--` for all enrolled paths in that
repo. The driver honors `GIT_INDEX_FILE`; an alternate index cannot borrow
metadata from the ordinary one. An older indexed manifest selects the older
retained preparation, never whichever cached version is newest. Unknown or
ambiguous metadata/source identities, exhausted catalog budgets, a held job
lease, failed jobs, wrong contracts/paths/repos and missing snapshots cause
failure without raw fallback. Source input must match exactly; the same canonical
pointer is accepted for an unhydrated checkout. Nothing uploads, encrypts,
installs filters or acknowledges a commit/push during cleaning.

## Independent index verification

Git can skip clean processing for unchanged files through its stat cache.
A clean driver alone therefore cannot prove that all staged references still
match a changed manifest. The independent guard inspects the actual index:

```sh
dracon-sync storage verify-index \
  --repo /path/to/repo --repo-id "$REPO_ID" \
  --metadata-root /private/storage-metadata \
  --manifest-path .dracon/assets.manifest
```

It rejects missing/raw/mismatched enrolled references and tombstones whose paths
are still indexed. It also checks **staged** effective attributes: enrolled paths
must use `dracon-storage`, that driver must be locally required, and tracked
paths using that driver must be enrolled. Unstaged attribute changes do not
substitute for the attributes going into the outgoing commit. Alternate indexes
are honored. The attribute query suppresses hooks/fsmonitor and ambient Git
repository/config overrides; errors leave the index and working files intact.
This is reference/attribute correspondence, not a backend availability or
independent-copy durability certificate.

Attribute queries refuse more than 100,000 paths or 16 MiB of NUL-delimited
input, cap stdout at 32 MiB and individual filter values at 1,024 bytes, and
share a 30-second I/O/process deadline. On Unix, query failures terminate the
owned process group, including descendants holding pipes after the parent exits.
Responses with duplicate/unknown paths, extra fields or missing terminators
fail without changing the index. Exceeding a budget blocks the check rather
than verifying only part of the repository.

Production setup must preserve unrelated attributes and hooks, compose Warden
routing and wire daemon staging to exact prepared versions. Configured commit
guard integration is described below; production setup, outgoing-history/push
coverage and worker gates remain unfinished.
The tests install required drivers only in isolated temporary repositories;
no preview command installs a filter or hook in the fleet. Cold-checkout metadata
import/hydration, S3 and automatic transfers also remain separate gates.


## Binding the commit guard (unreleased)

For an already prepared and verified index, explicitly bind the local guard:

```sh
dracon-sync storage setup-guard \
  --repo /path/to/repo --repo-id "$REPO_ID" \
  --metadata-root /private/storage-metadata \
  --manifest-path .dracon/assets.manifest

dracon-sync storage verify-configured-index --repo /path/to/repo
```

Setup verifies references and staged attributes before saving local configuration.
It pins the invoked Sync executable, private metadata root and manifest path,
then publishes the version marker last. Repeating the same binding is harmless;
a different active binding requires explicit maintenance. No driver, attributes,
hooks, upload or enrollment is installed by this command.

The source daemon now commits configured storage repositories through a direct
validated-tree path because libgit2 commits bypass pre-commit hooks. It holds an
owned Git index lock while validating and committing the immutable tree; foreign
locks and rejected index/worktree contents are preserved. Its own lock can be
recovered after process death. Ordinary repositories retain their existing path.
A storage driver without the explicit guard binding blocks this commit path.
The guard also detects `filter=dracon-storage` declarations in staged or HEAD
`.gitattributes`, including nested files and macros. A fresh clone therefore
requires its local binding even when it has no local filter configuration.
Staging attribute deletion cannot erase HEAD's declaration. Declarations reserve
storage routing even when no current asset matches; comments and quoted pattern
names containing the assignment do not activate it. Inherited filter settings
also require a local guard binding. A repository ID alone remains inactive.

Portable declaration detection checks ordinary attribute blobs without running
filters: at most 4 MiB per blob, 16 MiB across staged/HEAD attributes, a 64 MiB
ordinary owned index and a 30-second HEAD traversal deadline. Missing/corrupt
objects and exceeded budgets refuse the commit. This detects the local binding
requirement; full validation of every outgoing historical version is still a
separate unfinished gate.

The new Warden pre-commit template invokes the pinned guard after existing user
hooks, propagates failure and retains Warden encryption checks. Installing that
new template is a separate deployment step. Manual checks honor Git's alternate
index; daemon commits inspect the canonical index. A repo UUID alone does not
activate storage. These checks establish local index correspondence; they do not
certify backend availability. Outgoing-history/push protection, production
attribute/filter composition and enrollment remain unfinished. No installed
fleet binaries or hooks have been changed by this development work.


## Importing protected metadata on a cold checkout (unreleased)

A fresh clone has committed ciphertext, but no locally prepared metadata cache.
Explicitly bind the checkout to the backed-up repository ID, then import a
committed manifest with the authorized Warden keys:

```sh
git -C /path/to/cold-checkout config --local dracon.storageRepoId "$REPO_ID"
dracon-sync storage import-manifest \
  --repo /path/to/cold-checkout --repo-id "$REPO_ID" \
  --metadata-root /private/cold-metadata \
  --manifest-path .dracon/assets.manifest --revision HEAD \
  --policy-sha256 "$IMPORT_POLICY_SHA256" \
  --warden /absolute/path/to/dracon-warden \
  --identity-home /path/to/authorized-identity-home
```

The repository ID and policy digest come from operator-approved recovery
bindings, not from an untrusted manifest. The identity-home option uses existing
keys; importing never creates keys. Omit it to use Warden's existing identity
configuration. The default processing deadline is 30 seconds; `--timeout-secs`
sets a positive explicit override.

Import pins the selected commit and reads its ordinary-file Git blob, ignoring
unstaged manifest edits and alternate indexes. It checks the ciphertext's exact
length/digest and age header, decrypts into anonymous private temporary files,
then validates canonical bounded metadata and its repository ID before retaining
an approved cache record. Current support is for age-protected metadata only.
Neither an enrollment nor a backend identifier grants network access.

Imported records include ciphertext in their private identity, so independent
ciphertext versions of the same manifest coexist without replacing locally
prepared records. Existing version-1 record encodings/identities remain valid.
Limits and corrupted/ambiguous cache records fail closed; previous versions are
never evicted to make room. Interrupted approval/publication resumes from the
exact retained bytes. Timeout/cancellation terminates the owned Unix security
process group and releases the namespace lease.

This command creates only the requested private cache and its runtime markers.
It does not hydrate assets, change working assets/index, install filters/hooks,
activate guard bindings, upload or verify backend copies. Importing a manifest
also does not certify that the selected commit's pointers match it: use the
independent guard after explicitly binding the intended index. Packaged asset
hydration and outgoing-history validation remain separate unfinished gates.

## Recovering an exact asset into a private cache (unreleased)

After importing the selected commit's metadata, recover a named asset from an
operator-approved local copy:

```sh
dracon-sync storage restore-asset \
  --repo /path/to/cold-checkout --repo-id "$REPO_ID" \
  --metadata-root /private/cold-metadata \
  --manifest-path .dracon/assets.manifest --revision HEAD \
  --path assets/private-video.mp4 \
  --policy /path/to/operator.toml --backend recovery \
  --restore-root /private/restored-assets \
  --warden /absolute/path/to/dracon-warden \
  --identity-home /path/to/authorized-identity-home
```

The operator policy supplies the adapter and permitted security class:

```toml
[storage.backends.recovery]
type = "local"
root = "/absolute/existing/recovery-objects"
allowed_security = ["warden-encrypted"]
```

The selected backend must also be a required copy in that exact enrollment.
Omitting `--backend` selects its declared primary; there is no automatic fallback.
Repository overrides and manifests cannot supply adapter endpoints or grant
permissions. The local object store must already exist. S3 recovery is not yet
implemented. A non-sensitive enrollment requires an explicit `non-sensitive`
grant; recovering that asset needs no Warden adapter. The protected metadata
import still requires authorized keys.

Recovery pins one Git commit and checks its committed pointer against the
approved manifest. Working asset/manifest edits and alternate indexes do not
select the recovered version. It verifies payload length and SHA-256 while
fetching, and requires successful authenticated decryption for encrypted assets
before publishing private output. Missing keys, corrupt payloads, deleted
enrollments, mismatched references and failed decryption publish no asset.

The command prints the verified byte count and an opaque private file path.
It leaves the checkout, index, filters, hooks and daemon bindings unchanged.
Use the separate explicit hydration command below to publish working bytes;
this recovery command does not replace a
working pointer or overwrite concurrent edits. Existing recovery files are
verified and preserved, and conflicting content is refused. Versions are never
evicted to make room. Publication uses a create-only operation so a concurrently
created destination survives.

Default limits are 2 GiB per fetched payload, 1 GiB per recovered output, 4 GiB
of retained output and 10,000 retained versions per repository namespace.
Positive `--max-payload-bytes`, `--max-output-bytes` and
`--max-retained-bytes` override the byte limits; total retention must be at least
the per-output limit. These are recovery resource budgets, unrelated to a
forge's push limits. Anonymous transient files separately hold bounded payload
and decrypted output; failed attempts retain no published partial output.

## Hydrating the checked-out version (unreleased, Linux)

Use a source daemon with the storage guard and an already bound required clean
driver, or an isolated checkout outside daemon watch roots. The installed
0.113.92 daemon is not a deployment of this feature. Automatic enrollment and
filter installation remain unfinished; do not apply this preview to the fleet.

After importing metadata and explicitly setting up the matching guard, hydrate
one asset:

```sh
dracon-sync storage hydrate \
  --repo /path/to/checkout --repo-id "$REPO_ID" \
  --metadata-root /private/cold-metadata \
  --manifest-path .dracon/assets.manifest --path assets/private-video.mp4 \
  --policy /path/to/operator.toml --backend recovery \
  --restore-root /private/restored-assets \
  --hydration-root /private/checkout-hydration \
  --warden /absolute/path/to/dracon-warden \
  --identity-home /path/to/authorized-identity-home
```

The recovery options and grants match `restore-asset`. Hydration additionally
requires the selected commit to be HEAD, and the metadata root/path/repository
ID to match the configured guard. Inside the owned Git index lease it rechecks
placement and exact staged pointer/protected metadata correspondence before
changing working bytes. A historical version can be recovered privately with
`restore-asset`; check out that version explicitly before hydrating it.

The hydration root must be private, dedicated to this checkout and on the same
filesystem as the asset. Linux `/proc` and filesystem support for create-only
`renameat2` are required; unsupported operations fail without replacement.
Destination parents must already exist and cannot contain symlinks. Output is
an independent file, with mode 0600 (0700 for an executable Git entry), so edits
cannot mutate the recovery cache through a hard link. Local edits, symlinks,
hard links, unresolved indexes, changed parents and inconsistent references are
refused. Git index bytes, commits, filters and hooks remain unchanged.

Publication retains an immutable private intent and verified output, moves only
the matching working pointer into a private `original` backup, then publishes
without replacing an existing destination. A concurrently created operator file
wins. Capture conflicts restore the displaced file only if its working path is
still absent; otherwise both files are preserved for review. The command prints
the original backup path after success. No previous version is evicted.

There is a brief interval between pointer capture and publication when the
working path is absent. Process death can leave that interval open; durable
private records/backups and the owned Git lock survive. Rerun hydration, or add
`--resume-local` to the same command to use a matching previously authorized
local transaction without fetching or decrypting again. Local resume checks the
current commit, parent identity, private intent, staged references and retained
output digest; it never selects the newest unrelated record. Missing preparation,
corruption or local edits fail while retaining all files. On failure, inspect the
reported private roots before manual recovery; no force/overwrite mode is supplied.

Hydration retains at most 10,000 transaction versions per repository namespace.
The existing output/retention byte flags also bound transaction output and
original backups, separately from the recovery cache; private metadata records
and interrupted record spools have their own bounded count/size. An oversized
or full store refuses further growth. This preview does not provide automatic
startup reconciliation, S3, smudge hydration or an independent-provider durability
certificate. Complete outgoing-history/push checks remain unfinished.
