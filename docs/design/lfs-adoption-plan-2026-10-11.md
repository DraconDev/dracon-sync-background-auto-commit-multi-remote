# Selective automatic Git LFS adoption plan

Date: 2026-10-11. Status: proposed implementation plan, not deployed.

## Decision and scope

Keep Dracon Sync a seamless commit/push orchestrator. Adopt standard Git LFS
clients and evaluate a maintained LFS API server with configurable S3-compatible
storage (OVH first). Do not implement a second LFS protocol or require every repo
to use external storage. The server candidate is not selected or certified yet.
This document changes the proposed direction, not live policy or existing recovery.

The earlier storage roadmap and representation decision contain useful security,
recovery and journal work, but their Sync-specific transfer/hydration architecture
is not automatically the implementation target. Retain existing recovery code
until equivalent standard-LFS recovery is demonstrated. Stop expanding the custom
path during evaluation; do not remove pilots, keys or objects prematurely.

## Why size alone is not the rule

A 200 MiB repo with 20 ordinary images can remain entirely in Git. Submodules
partition projects, not asset growth, and are not required for this feature.
GitHub's 2 GiB limit is per push, not total history. Local object database size,
reachable history, pending push estimates, working files, nested totals and bucket
bytes are different measurements. Parent and child rows must not be double-counted.
Accidental caches/build output need hygiene, not relocation to a billable bucket.

Size pressure activates asset selection, not migration of an entire repository.
For illustration only, configurable initial pilot settings could be:
- soft own-repo Git history budget: 1 GiB;
- minimum individual eligible asset: 20 MiB;
- individual safety trigger: approaching the configured forge blob limit;
- churn trigger: repeated eligible asset versions consuming a configured budget.
These are proposed tuning values, not current defaults or provider guarantees.
Use measured compressed history plus candidate raw bytes as a conservative signal,
not an exact pack-size prediction. If history measurement is unavailable, report
unknown; retain individual-file safety gates rather than declaring the repo small.
Do not wait for a rejected push to activate the policy.

## Placement precedence and security

Apply in order, on stable file snapshots:
1. Existing Git ignores and approved generated-output exclusions remain effective.
2. Warden classification and operator deny rules win over all storage thresholds.
3. Existing enrolled LFS paths retain their representation.
4. Approved non-sensitive asset paths are candidates for automatic enrollment when
   repo pressure, projected growth, churn or an individual-file limit warrants it.
5. Other admissible files stay in Git.

SQL dumps, SQLite/database files (including WAL/SHM), credential exports, session
stores, archives with unknown contents and unclassified payloads are not eligible
for automatic asset upload. Do not trust extensions alone: verify content/type and
classification; images can contain sensitive material too. Do not ban harmless SQL
source/migration text just because it ends in .sql. Sensitive text remains Warden's
responsibility. Databases require a separately approved consistent backup policy;
copying a live database into LFS is not a database backup solution.

Unknown/sensitive large files remain held with a precise actionable concern, not
uploaded, silently ignored, or allowed to bypass existing size/security safeguards.
A clean text scan alone is not sufficient proof that arbitrary binaries are safe.

## Automatic routing and sticky enrollment

The operator enables the feature and approved bindings once. Routine eligible
changes need no manual `git lfs track`, upload command or per-file question.
Sync calculates eligible paths, writes narrowly scoped managed .gitattributes
entries and stages those entries with their pointers as one coherent commit.
Avoid broad extension rules accidentally enrolling small or sensitive files.
Escape Git attribute paths correctly; cover spaces, special characters and case.

Once a path becomes LFS-managed it stays managed even if size later falls, avoiding
representation oscillation. Repository pressure also uses hysteresis. Renames,
deletions and re-created paths require tested enrollment semantics. Existing
uncommitted/staged operator work must not be overwritten. Use a repo transaction
lock and revalidate snapshots/attributes/index before committing; retry safely
following edits, crashes or another Git process. Never stage a stale pointer.

Conversion of an existing tracked eligible file is an ordinary forward commit:
bytes remain in the working tree, Git gains a pointer. Old commits remain raw and
large. No automatic `git lfs migrate`, rebase, force-push or history rewrite.
Legacy slimming remains separate operator-approved backed-up maintenance.

## Responsibilities and configuration

- Git/GitHub/GitLab: commits and standard LFS pointers.
- git-lfs client: local object cache, clean/smudge, transfer and pre-push integration.
- maintained LFS server: authenticated API and object access; S3 backing alone is
  not an LFS API. Qualify its object-existence, authorization and durability semantics.
- configurable backend: endpoint, region, bucket and optional supported namespace,
  TLS/addressing options; credentials resolved privately, never in Git or argv.
- Sync: policy selection, safe setup, bounded scheduling, reporting and retries.
- Warden: classification/encryption and security hooks; independent of LFS hosting.
- asset publisher: separate explicit publication of approved browser assets.

Proposed configuration must define global enablement, approved backend aliases,
pressure/file/churn thresholds, path allow/deny rules and per-repo opt-out.
Repos may select operator-approved aliases, not inject arbitrary URLs or credentials.
Each per-repo field needs an override, merge semantics and behavioral tests;
reuse the existing override coverage tripwires. Configuration examples are only
added as runnable TOML once the schema is implemented, not advertised today.

Use one shared LFS endpoint for GitHub and GitLab, with repo-scoped authorization.
Configure an endpoint hint without credentials; validate committed .lfsconfig as
untrusted before using it. Private source objects must not become publicly readable.
The existing public OVH asset namespace is not automatically safe for source assets:
choose a private bucket or demonstrate enforceable private namespace isolation.
Same payload need not upload twice to the shared endpoint, but one backend is not
two independent backups. Preserve historical objects; no age-only deletion of
objects referenced by old commits. Backup metadata/auth config as well as objects.

## Warden and hook integration

Most selected assets should not need the Warden text-encryption filter. Inspect
actual classification and effective attributes, rather than assuming no overlap.
Git chooses one effective filter; stock LFS and Warden cannot simply be stacked.
Files requiring both are held out of automatic enrollment until a reviewed,
versioned composition and historical-key recovery test exists.

Current global Warden pre-push chains local hooks before its own scan. Installing
LFS into that chain could upload bytes before later rejection. Design a supported
hook dispatcher: required classification/payload security authorization first,
LFS upload next, Git push last. Preserve stdin refs and all existing protections.
Pointer scanning is not payload scanning. Direct manual Git operations must receive
appropriate equivalent checks; managed daemon safety alone is insufficient.
Do not overwrite global hooks with `git lfs install` or rely on `--no-verify`.

The Doomtap pilot currently configures an optional custom asset filter and raw-Git
fallback on credential/upload failure. Retire that workflow only after verified
LFS import and recovery; never run both drivers over the same path.

## Seamless daemon and consumer workflows

Producer: classify snapshot -> select/enroll -> LFS local clean/cache -> coherent
commit -> verify security authorization -> LFS transfer -> push Git to each forge.
Missing LFS binaries, credentials, object upload or required-hook failures create
retryable concerns with no raw-byte fallback. They must not stall unrelated repos.
Retain local files/cache and unpublished commits; resume after restart. A local
commit can legitimately precede upload, but remote Git success must not precede
successful required object publication. Existing commit history need not be undone
on transfer failure.

The 100 MiB auto-staging gate needs an LFS-aware exception only for approved paths
whose effective driver, pointer and local payload are verified. Do not remove the
limit globally. Existing Git-only blob limits and Warden safeguards stay intact.
Network work gets bounded per-repo scheduling/backoff; unchanged checks use local
state rather than doing bucket requests on every status/diff. Report Git push,
LFS transfer and recovery/backup evidence separately; historical receipts do not
prove present availability. Object/cache budgets and retention must prevent disk
exhaustion without deleting the only unpublished copy.

Consumers: with git-lfs installed and endpoint access configured, clone/pull from
either forge obtains Git pointers then actual assets. Without tooling they may get
pointers; Git does not auto-install LFS or hooks from a clone. Seamlessness means
packaged prerequisites plus an idempotent bootstrap/doctor, not pretending every
arbitrary plain-Git machine works. CI and developer onboarding share the bootstrap;
interactive credentials are not acceptable for unattended Sync. Document offline,
partial-download and retry behavior. Forge ZIP archives/previews may not hydrate
external LFS objects; advertise a tested clone/bootstrap path, not unsupported ZIPs.
Browsers use public published URLs, not developer LFS authentication.

## Phased implementation and acceptance

1. Inventory: map Warden filters, asset candidates, churn, custom pilots, pending
   pushes and accidental history across representative small and large repos.
   Produce a read-only routing report. No production mutation.
2. Server qualification: shortlist maintained S3-backed LFS servers; record
   maintenance/security status, licensing, repo authorization, OVH compatibility,
   transfer limits, backups and actual storage/egress costs. Select based on an
   isolated standard-client test, not a claim that OVH supports LFS directly.
3. Isolated automation: implement policy/overrides, staging exception, sticky
   enrollment, hook dispatcher, bootstrap and independent per-repo retry behavior.
4. Authorized live pilot: one representative game, private storage first. Measure
   costs and test cold recovery before changing fleet defaults or retiring pilots.
5. Selective rollout: large eligible repos only; update README with installed
   workflow, diagnostics and consumer commands. Retain ordinary Git fast path.
   Legacy history cleanup remains a separate approval and recovery exercise.

Mandatory gates before claiming production automation:
- 200 MiB/20-small-image fixture remains ordinary Git with no bucket dependency.
- File-limit, repo-pressure and churn fixtures trigger narrow asset enrollment;
  database/secret/unknown-content fixtures never upload; migration SQL remains
  subject to normal text policy rather than blanket database exclusion.
- Attribute precedence, escaped paths, overrides, opt-out, sticky paths,
  rename/delete/recreate, changed snapshot and competing index tests pass.
- Security refusal happens before object upload; manual Git and daemon pushes
  preserve hooks. Warden text round trip and large LFS asset coexist.
- >100 MiB approved fixture stages a valid pointer; unapproved oversized fixture
  remains refused. Raw fallback is impossible on every failure path.
- Push to GitHub and GitLab using shared endpoint; fresh isolated machines clone
  from each, authenticate, restore exact digests and build. Check old versions.
- Missing tool/credentials, server outage, corrupt/missing object, disk full,
  interrupted upload, restart and one-forge failure retain recoverable state;
  unrelated repo continues syncing. Backend success with Git rejection is safe.
- Independently backed-up objects + server metadata restore on a replacement
  endpoint, without original producer cache. Test credentials/key rotation.
- CI/bootstrap, submodule/nested and standalone shapes work; no recursive tests.
- Live endpoint/object ACLs enforce private access; public publication is explicit.
- Repo tests/build/clippy/security checks pass for implementation changes, with
  packaged-install tests and documented supported versions. No release claims
  based only on local fixtures or merely successful Git pushes.

## Planning boundary

This plan does not enroll assets, provision a server, upload private bytes, change
live filters, relax exclusions or rewrite history. Open implementation decisions:
server selection, measured threshold tuning, payload classification coverage,
private backend binding and supported encrypted-LFS overlap. These are bounded
engineering/pilot gates, not evidence that seamless production routing exists now.
