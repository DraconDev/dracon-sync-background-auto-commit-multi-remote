# Standard LFS adoption: implementation/qualification report

Date: 2026-10-11. Status: isolated policy primitives implemented; production routing and live qualification remain unfinished.

## Decisions taken

- Target protocol: standard Git LFS v1 with SHA-256 identity. The existing
  strict pointer encoder at `dracon-sync/src/storage_core/reference.rs` is
  preserved as the pointer contract for any future Sync-managed route, but
  is not connected to the stock `git-lfs` client or a maintained server.
- Fast path: ordinary small repositories (illustrative budget 1 GiB local
  Git history, 20 MiB individual files) remain in Git with no bucket
  dependency. No automatic asset enrollment.
- Pilot candidate: `polis` (`web/games/wip/polis` in the sibling
  `dracon-platform` checkout), not a qualified or enrolled pilot. The prior
  inventory reference is outside this report's audit boundary; the parent
  utilities `web` symlink cannot serve as durable inventory evidence. Neither
  asset approval nor present forge privacy is established by this report.
- Backend: configurable. OVH S3 endpoint (`s3.uk.io.cloud.ovh.net`) is the
  documented default; private-source traffic must use a separate, private
  bucket or namespace from the public asset bucket `dracon-master`.

## What is not yet implemented

- No production LFS server is installed or qualified. Gitea is selected
  below as the first isolated-test candidate, not an approved deployment.
  The earlier PATH inventory did not find `git-lfs`, `gitea` or `forgejo`;
  this is a non-exhaustive local install search, not a remote-host audit.
- No private source OVH binding or scoped IAM principal has been qualified
  by this work. This is not proof that none exists elsewhere. The public
  asset bucket `dracon-master` is unsuitable for private source LFS traffic.
- Deployment host: none qualified or identified for LFS deployment. An
  S3 endpoint is storage, not an authorized LFS API deployment host.
- The Warden global pre-push hook (`~/.config/git/hooks/pre-push`) chains
  foreign and repository-local hooks before its own checks. A stock LFS
  pre-push would therefore upload bytes before any refusal. This is a
  blocker for automatic LFS routing and is not yet fixed.
- No daemon-side standard LFS integration exists. `src/storage_core`
  also contains capture, journals, hydration, staging, security, transfer
  and recovery modules for the existing custom pipeline. These are retained;
  their existence does not prove stock-client or LFS API interoperability.
- No recovery story is implemented. The previous design plan
  (`lfs-adoption-plan-2026-10-11.md`) requires an independent backup of
  LFS objects and required server metadata, which is not present.

## Ranked server candidates (desk research only)

Public upstream material retrieved 2026-10-11 supports this two-candidate
shortlist. Gitea's binary was subsequently exercised in a disposable loopback
fixture (below); no production install, stock-client/server version pair or
OVH interoperability is certified here.

1. **Gitea 28.1.0: first isolated-test candidate.** The upstream release
   API reports non-prerelease `v28.1.0`, published 2026-10-06. Its documented
   built-in LFS API, S3-compatible `minio` storage driver, configurable
   endpoint/region/bucket/addressing/TLS, and database/config backup procedures
   make it a concrete candidate instead of a bespoke protocol service.
   Tagged source checks repository permissions and repository-bound LFS metadata.
   This is evidence for evaluation, not a comparative security certification.
2. **Forgejo 15.0.9 LTS: alternative.** The upstream release page lists this
   line with support through 15 July 2027. Its storage docs describe an
   S3-compatible driver without MinIO-specific requirements. The explicit
   maintenance window is attractive; configuration must be verified against
   the chosen version rather than assuming latest documentation matches LTS.

Both bring an additional forge/database service even if commits remain on
GitHub/GitLab. That operational cost must be justified on existing authorized
infrastructure; it does not authorize new paid resources or public exposure.
A matching private repository record on the service is needed for repo/OID
associations and authentication. Bucket bytes alone cannot restore those
associations. Independently preserve server DB, permission/auth records,
configuration/secrets and object bytes; a server dump does not prove remote
S3 objects were backed up. Prefer proxy serving during qualification, leave
TLS verification enabled, and use separate least-privilege producer/consumer
principals with credentials outside Git, URLs and argv.

Required isolated qualification includes stock-client batch/upload/verify/
download, OVH signing/addressing/region compatibility, refusal of anonymous
and cross-repository known-OID access, interruptions/retries, and independent
DB/config/object restore. Neither candidate's documented capability proves
this shared endpoint works with both external forges or unattended consumers.

Upstream evidence:
- <https://api.github.com/repos/go-gitea/gitea/releases/latest> (release
  fields independently retrieved by parent; a mutable latest endpoint).
- <https://github.com/go-gitea/gitea/releases/tag/v28.1.0>
- <https://docs.gitea.com/administration/config-cheat-sheet>
- <https://docs.gitea.com/administration/git-lfs-setup/>
- <https://docs.gitea.com/administration/backup-and-restore/>
- <https://raw.githubusercontent.com/go-gitea/gitea/v28.1.0/services/lfs/server.go>
- <https://forgejo.org/releases/> (version/support window independently
  retrieved by parent).
- <https://forgejo.org/docs/latest/admin/setup/storage/>
- <https://forgejo.org/docs/latest/admin/command-line/>
- <https://codeberg.org/forgejo/forgejo/raw/tag/v15.0.9/services/lfs/server.go>

Deployment host remains **none qualified / authorization details unknown**;
the OVH S3 hostname is not such a host. Do not deploy from this report alone.

## Isolated local policy milestone

`src/lfs_policy.rs` exports a non-mutating evaluator with explicit tuning and
per-repository override resolution. It checks pressure, file size and churn,
sticky representation, classification, effective-filter conflicts, opt-out and
approved-binding evidence. Invalid thresholds fail closed. No deployment
threshold defaults are established by the illustrative test settings.

Bounded checks run successfully:

```sh
timeout 180 cargo test -p dracon-sync --lib --locked lfs_policy::tests
# 11 passed, 0 failed
timeout 180 cargo clippy -p dracon-sync --lib --locked -- -D warnings
# passed (library only)
```

A read-only reviewer found no defects in this isolated milestone and requested
sticky-state cross-product coverage before integration. The added test checks
160 effective-LFS combinations across ledger presence, classification, binding,
opt-out and missing/valid/invalid/disabled tuning; each retains LFS or holds.
The reviewer did not independently rerun the tests or approve production use.

These tests use typed synthetic evidence, not real file classification or Git
repositories. The 200 MiB/small-image case models repository history and a 10 MiB
candidate. SQL source is represented by `SourceText`; no SQL-content classifier
is implemented. The >100 MiB case is a selection decision, NOT a staging-size
exception. A positive decision does not authorize uploads. There is no TOML
schema integration, attribute enrollment, production dispatcher, consumer
bootstrap, transfer scheduling, stock-client compatibility test or live
verification yet.
The full verification contract and tasks 2–5 remain unfinished.

## Real-file classification adapter milestone

`src/lfs_classify.rs` builds evaluator evidence for real filesystem
snapshots. Classification is deliberately conservative: a path is an
approved asset only when its extension appears in the caller-supplied
allow-list, while sensitive and database indicators always win regardless
of the allow-list. Extensions alone never promote an unknown payload.

Precedence is fixed and documented:
1. Sensitive — file name in `{".env", "id_rsa", "id_ed25519",
   "secrets.json"}` or extension in `{"pem", "key", "age", "env"}`.
2. Database — extension in `{"db", "sqlite", "sqlite3", "sqlitedb",
   "dump", "wal", "shm"}` or a 16-byte `SQLite format 3\0` magic header
   on the file (defends extensionless database payloads).
3. SQL source — `*.sql` is treated as `SourceText`, not an asset.
4. Reviewed asset allow-list — e.g. `png`, `jpg`, `mp4`, `bin`.
5. Anything else — `Unknown` and held; raw bytes are never enrolled.

Snapshots build a real `Evidence` whose `bytes` value is the file's
`metadata().len()` (no synthetic byte counts), so size-driven routing
always reads the real measurement. Override resolution reuses the policy
module's `with_override` so per-repo tuning flows through the same code
path as the typed-evidence tests.

Bounded checks run successfully against real tempdir fixtures
(unique per-test, removed on drop):

```sh
timeout 180 cargo test -p dracon-sync --lib --locked lfs_classify::tests
# 8 passed, 0 failed; 0 ignored
```

The 8 new tests cover: a 200 MiB small-image repo (200 × 1 MiB files at
1 MiB) staying in Git under history pressure; large `.pem` and `.db`
refusal regardless of size or pressure; SQLite-magic refusal for an
extensionless payload; large `.sql` migrations staying in `SourceText`;
unknown `.zip` held; narrowly-scoped >100 MiB approved asset selection
(not a staging-size exception); sticky `RetainLfs` surviving real file
shrinkage (sparse-rewritten to 1 byte, representation preserved);
per-repo override moving the individual-trigger threshold and changing
the decision on a real 60 MiB file. All sizes use sparse writes so the
fixtures stay disk-light while `metadata().len()` reports the logical
size under test.

This adapter remains read-only — it never opens sockets, runs Git
commands, mutates the worktree, or stages anything. It does not prove
daemon wiring, stock-client compat, server compatibility, or live
enrollment; those still require the six concrete unblockers below and
remain unfinished.

## Isolated dispatcher primitive

`scripts/lfs_dispatcher_prototype.py` is a callable, non-installed prototype,
not the global Warden hook or a stock-client integration. It requires explicit
absolute executable paths, at least one guard and transfer command, bounded
ref input and one total deadline. Every guard must succeed before any transfer
command is invoked. Each command receives identical push arguments and ref
bytes; failures stop the sequence with separate security/transfer attribution.
Recursion is refused and timed-out process groups are terminated. No retry or
raw-byte fallback is implemented. There is deliberately no CLI/install entrypoint.

```sh
timeout 30 python3 -m unittest discover -s dracon-sync/scripts \
  -p test_lfs_dispatcher_prototype.py -v
# 12 passed, 0 failed
```

Reviewer feedback was addressed: NUL-bearing argv/environment values fail
configuration validation before any hook runs, and phase/shared-deadline tests
use deterministic clock/process doubles rather than interpreter-start timing.
One real security-timeout test remains; it requires no hook marker timing.
The reviewer inspected source only and did not rerun tests. Process groups
are bounded here, not a sandbox: descendants that leave their group cannot be
contained by this primitive.

Fixtures use fake local hooks and marker files, with no credentials, Git pushes,
real Warden checks or object uploads. They cover guard refusal before every
transfer, exact args/ref replay, multi-ref/SHA-256/deletion syntax, missing
executables, malformed/oversized input, recursion, transfer failure, shared
deadline and an independent successful invocation after refusal. The latter
is not proof of daemon fairness or unrelated live-repository progress.

Future integration must supply actual Warden/history checks plus snapshot/
repo/path/OID-bound payload authorization and stock LFS transfer commands.
Public Python arguments do not establish approval of commands or payloads.
The live hook-order defect remains unchanged, and tasks 2–5 remain unfinished.

## Warden source-only hook ordering milestone

Designer launch returned `Unknown agent: Designer`; the requested inline
fallback selected the smallest change: preserve foreign-before-local chaining,
exact arguments/ref replay and legacy-Warden recursion skipping, but invoke
both chained hooks only after every ref passes the existing checks. Source
changes are in `dracon-warden/src/main.rs` (`PRE_PUSH_HOOK`) with regressions in
`src/tests.rs`. Ref buffering/temp creation also now refuse on failure rather
than allowing hooks to run after incomplete input capture.

```sh
timeout 180 cargo test -p dracon-warden --locked pre_push_hook
# 33 passed, 0 failed
timeout 180 cargo clippy -p dracon-warden --locked -- -D warnings
# package check passed
```

A read-only reviewer found no issues in the committed source diff and reviewed
parent test/clippy logs; no checks were independently rerun. Fault-injection
regressions for buffer/temp creation and foreign-hook failures remain to add.

Tests run the rendered hook in disposable Git repositories. A synthetic secret
refuses before either fake upload hook; a later deletion ref refuses before
hooks even if the first ref is clean. Clean multiple refs replay identical
stdin to foreign then local hooks. Existing chaining failure, history, author,
text/binary secret and legacy-hook cases pass. This is real hook ordering, NOT
LFS object-content authorization, transfer bounds, or stock-client compatibility.
No setup/harden/install command was run; the live global hook is unchanged.

## Isolated stock Git LFS pointer/filter qualification

Git LFS 3.8.0 linux-amd64 was downloaded to an isolated temporary directory;
its release archive matched GitHub release API SHA-256
`e455e00f15d9b95661b8d53498ffb0c3367962cf1ec73c31ab7369516cd6ab8d`.
No installer was run, and no system PATH, live Git config, hooks or filters
were changed. Filter settings below apply only to disposable test repositories. Source: <https://github.com/git-lfs/git-lfs/releases/tag/v3.8.0>.
This checksum match is transport/artifact verification, not an independent
publisher-signature validation.

```sh
GIT_LFS_BIN=/absolute/path/to/verified/git-lfs \
  timeout 120 python3 -m unittest discover -s dracon-sync/scripts \
  -p test_stock_lfs_pointer.py -v
# 7 passed, 0 failed against stock Git LFS 3.8.0
```

The command fails if the required client is absent or has a different version.
Tests isolate HOME/global/system Git configuration. Nonempty binary and
>100 MiB sparse temporary fixtures produce exact canonical SHA-256/size
pointers that stock strict validation accepts; malformed pointers are refused.
A disposable Git repo uses stock clean filtering to stage an exact pointer
for a >100 MiB fixture while preserving its worktree bytes. A missing required
clean executable refuses staging with no raw blob in the index. No fixture
is committed or uploaded, and no existing checkout config is changed.
These results do NOT verify Sync's >100 MiB size-guard exception or a server.

A stock smudge fixture restores exact binary bytes/digest from an independently
copied object directory in another disposable Git repo. It does not share a
producer cache setting or hardlinks. This is local-object compatibility, NOT
fresh-forge clone hydration, independent server backup, metadata restore,
historical checkout or network outage recovery.

The first run exposed an important representation difference: stock LFS
represents empty files as empty content, not a zero-size pointer. The generic
Rust encoder supports zero-size pointers, but integration must honor the
stock empty-file behavior rather than claiming byte-for-byte pointer output
for empty files. This is standard empty-file handling, not transfer-error raw
fallback. Live consumer bootstrap and server compatibility remain unqualified;
tasks 2–5 are still open.

## Workspace gate checkpoint (not a pass)

`timeout 180 cargo test --workspace --locked` exited 124 after completed suites
reported zero failures; the run reached `scanner_stress` but did not complete
all workspace tests. The suite is NOT claimed passing. A bounded isolated
`timeout 90 cargo test -p dracon-security --locked --test scanner_stress`
subsequently passed (1 test, 0 failed). This rules out a reproducible hang in
that isolated test, not a workspace-wide failure. A subsequent 360-second
workspace run also exited 124, later in Sync unit tests. Do not repeatedly
increase the whole-suite deadline: isolate the slow Sync test/ambient-hook
interaction, then choose bounded per-package runs before the final full gate.
The named last-running Sync test subsequently passed in isolation:
`timeout 75 cargo test -p dracon-sync --bin dracon-sync --locked
sync::tests::test_sync_repo_mirror_push_failure_second -- --exact`
(1 passed, 0 failed). An initial library-only invocation matched zero tests;
only the binary invocation above is evidence. No reproducible hang is proven.

Other workspace gates now pass:

```sh
timeout 180 cargo clippy --workspace --locked -- -D warnings
# exit 0
timeout 120 cargo deny check
# exit 0; advisories/bans/licenses/sources ok (diagnostic warnings retained)
timeout 300 cargo build --release --locked
# exit 0, optimized build finished
```

Per-package gate evidence (workspace `cargo test --workspace --locked`
continues to time out at 180–290 s in dev mode; release mode compiles
but is not faster than the bounded dev-mode windows, so the workspace
gate is documented as not passing in the standard `cargo test
--workspace --locked` form):

```sh
timeout 90 cargo test -p dracon-sync --lib --locked
# 144 passed, 0 failed, 13 ignored, finished in 48.95 s
timeout 90 cargo test -p dracon-warden --locked
# 2 + 206 + 18 binary suites, 0 failed, finished in ~63 s
timeout 90 cargo test -p dracon-system --locked
# 12 passed, 0 failed, finished in 0.33 s
```

These checks do not qualify a server, packaged installation, live hooks, or
consumer bootstrap; the full workspace test gate still needs a successful run.

## Disposable maintained-server API milestone

Gitea 28.1.0 linux-amd64 was downloaded into a temporary directory and its
binary matched release API SHA-256
`ea93cfa4a300564ac457d3903469fea49ec5a4da37d0cebaa435f8e329ea64d0`.
`gitea --version` reported 28.1.0, built with go1.27.1. Source:
<https://github.com/go-gitea/gitea/releases/tag/v28.1.0>. This is a checksum
match, not independent signature verification or deployment authorization.

```sh
GITEA_BIN=/absolute/path/to/verified/gitea \
  timeout 180 python3 -m unittest discover -s dracon-sync/scripts \
  -p test_gitea_lfs_api.py -v
# 1 API lifecycle test passed, 0 failed
```

The Linux-only harness starts SQLite/local-object Gitea on 127.0.0.1, proves
its child owns the selected listening socket through `/proc`, disables external
proxy/HTTP redirects in the test client, and uses bounded commands/requests.
Test HOME/Git config, database, private repository records, credentials and
object bytes are disposable and outside tracked trees. Generated passwords
and tokens are captured, not printed or passed through argv; failure diagnostics
redact configured secret/token/password fields. Gitea initializes its own
identity in that isolated Git config, not the operator's config. Server shutdown
is registered for normal/error cleanup and outer SIGTERM; no service remains
installed. No live filters, hooks, forge repos, OVH resources or paid services
are touched.

The test creates two private synthetic repo records, verifies authenticated
LFS batch/upload/download with exact SHA-256 bytes, refuses anonymous batch
and object reads, and refuses a known OID download from the second repo before
association. The fixture principal has broad test-only permissions; this does
NOT certify production least-privilege IAM. HTTP is loopback-only; production
still requires TLS and scoped authentication. Only the standard basic-transfer
API is exercised through Python, not a stock-client upload/download or Git push.

Initial fixture attempts exposed two bootstrap assumptions: Gitea needs a
writable isolated Git config (not `/dev/null`), and a ROOT_URL subpath requires
reverse-proxy handling. The harness now uses a direct loopback URL with actual
listener ownership verification rather than guessing that a random URL prefix
isolates a competing service. LFS requests include both required media headers.
These fixes apply to the disposable harness, not live infrastructure.

Stock-client/server round-trip (next loopback milestone)

Stock Git LFS 3.8.0 round-trips an object through the disposable server with
the same listener-ownership guarantees and redacted credential handling. The
fixture has no forge commits, no production-cache shortcut, and no shared
LFS object dir between producer and consumer. A consumer starting with an
empty `.git/lfs` cache, separate HOME and a per-repository access token pulls
the exact bytes/digest from a fresh download and stores them in its own
cache. A bogus OID fails without fabricating content; the failed-smudge
stdout is either empty or the input pointer, never restored bytes.

```sh
GITEA_BIN=/absolute/path/to/verified/gitea \
GIT_LFS_BIN=/absolute/path/to/verified/git-lfs \
  timeout 180 python3 -m unittest discover -s dracon-sync/scripts \
  -p test_gitea_lfs_api.py -v
# 2 tests passed, 0 failed (lifecycle API + stock-client round-trip)
```

Reviewer feedback on this fixture closed one weaker assertion: cross-
repository refusals now require the expected 404 object-error code, not just
"any error". The review noted source-only inspection of the committed
fixtures; no live test rerun occurred. The same loopback harness does not
certify TLS, scoped production IAM, OVH, or fresh-forge clones, and the
smudge-on-failure assertion is the strongest server-compatibility evidence
this goal ships.

## Concrete unblockers

1. Install a maintained standard LFS server shortlist candidate on an
   authorized host. Recommended: deploy a single candidate under an
   approved service unit and review its security/maintenance posture
   before broader rollout.
2. Create a private OVH bucket and a dedicated IAM principal with
   read/write scoped to the pilot repository. Do not reuse the public
   asset credentials.
3. Define and install an independent backup target for LFS objects and
   the server's own metadata. Verify restore by SHA-256 OID on a fresh
   host.
4. Implement repo/path/OID-bound LFS payload authorization before any
   foreign or local pre-push hook, then qualify and authorize live hook
   installation. Existing Warden checks now precede chaining in the source
   template, but do not inspect LFS payloads behind pointer blobs.
5. Install and configure the `git-lfs` client on the producer host
   (this one) and on a separate, fresh consumer host. Both must reach
   the LFS server using the dedicated credentials.
6. Select a representative private game and approve a single new
   non-secret binary fixture under a deliberate path
   (e.g. `lfs-pilot/fixture.bin` ≤ 1 MiB in `polis`). No existing
   tracked file is converted and `git lfs migrate` is not used.

## Boundaries preserved

- No public asset contract was changed.
- No live filters, hooks, or remote configuration were modified.
- No production credentials were opened, used, or stored in this tree.
- No history rewriting, retention deletion, or fleet-wide enrollment
  was performed. The existing custom storage module is untouched.

## Suggested next step

After the unblockers above, the next goal can install and qualify the
shortlisted server, implement the daemon-side LFS path, fix the Warden
hook order, and run a forward-only pilot on `polis` with the chosen
fixture. The verification contract and bounded test fixtures defined in
the adoption plan remain the acceptance gates.
