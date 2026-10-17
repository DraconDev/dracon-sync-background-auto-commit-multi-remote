# Outgoing raw-blob accounting implementation — 2026-10-18

Implements the pressure-accounting list item of the 2026-10-17 routing
contract. No production deployment, credential changes or hook bypass.

## Executed path

`bucket_runner_bridge::prepare` measures before serializing its publish
request and before spawning the upload-capable runner. It supplies measured
bytes to the existing validated shared-policy budget instead of zero.
Errors propagate through the existing managed-repository blocked outcome;
there is no zero or legacy fallback on a managed accounting failure.

`bucket_runner_bridge/outgoing.rs` computes a conservative raw-blob upper
bound, not compressed pack size, repository disk size or whole worktree size.
All Git children use captured-output/process-group ownership with concurrent
stdin/output draining. Accounting strips inherited GIT_* injection, disables
lazy promisor fetch, optional locks, fsmonitor and replacement refs. No Git
object writes, pack creation or network calls are used by measurement.

## Selection and measurement

- Snapshot HEAD, index entries, tracking tips and filter-disabled porcelain
  status. Disable configured clean/process filters for status; no upload
  filter is invoked to discover dirty paths. Untracked enumeration honors
  Git ignores and does not traverse nested repositories.
- Union outgoing HEAD objects absent from each known destination tip. Query
  object types/sizes in one cat-file batch; sum unique blob IDs, not trees
  or commits. Deletions do not erase outgoing historical bytes.
- Include locally configured origin plus every operator-policy destination,
  minus explicit per-repo remote exclusions. This is deliberately a
  conservative superset: mutable visibility/quota state must not remove a
  potential destination. In particular a public visibility cache can enable
  Codeberg even with auto_create=false. No remote is created or contacted.
- For an unknown non-upstream/new destination, enumerate ALL reachable HEAD
  objects as a conservative bound. This does NOT claim the remote is empty.
  For a declared configured upstream whose tracking ref is missing, refuse
  rather than disguise lost tracking evidence. Corrupt/unreadable Git objects
  and malformed input also refuse. This distinguishes safe upper bounding
  of new mirrors from silently accepting missing expected input.
- Prospective selection reuses the staging module's shared pattern/size gate
  (`should_stage_entry_with_blob_size`) plus the same untracked toggle and
  untracked/global+repo auto-commit exclusions. Worktree raw bytes represent
  restaging; retained staged-only candidates are conservatively included.
  Unique raw blob hashes are computed through hash-object --no-filters
  WITHOUT -w. Text normalization may make this an overestimate, never an
  assumed ciphertext bound.
- Never traverse symlink ancestors. Symlink blob bytes are link text, never
  the target file. Skip directories/gitlinks and nested roots. Read regular
  files through O_NOFOLLOW/nonblocking descriptors; verify descriptor identity
  before/after reading and path identity afterward.
- Active arbitrary filters or working-tree encodings have no proven output
  bound here: dirty candidates with those attributes refuse. No plaintext is
  written into the Git object database to infer encrypted/filtered size.
- Recheck HEAD/index/tips/status and all measured file identities before
  returning the estimate. Existing manifest/source/staged checks still
  protect the publication and commit phases after this pre-routing snapshot.

## Conservative choices and limits

Unlike an exact staged tree, the soft pre-routing estimate can count more
than the next batch: include all committable candidates, all potential
policy destinations and uncertain surviving staged versions. This cannot
weaken guards or authorize an ineligible asset. No raw managed source is
removed or tracked asset retired. Per-blob and pack guards are unchanged.

The entire measurement has a 60-second deadline. Each captured output has
the existing 64 MiB limit. Unique outgoing objects, index entries and measured
candidates each have a 20,000-entry limit; configured filters have a 128-name
limit. Regular candidate reads cap at 16 MiB. Checked integer addition
rejects overflow and values outside JS safe-integer range. Exceeding a limit
is a visible retryable refusal, not zero or a partially measured success.

Future filter-bound support or higher limits need independent safety tests;
these conservative refusals are documented implementation boundaries, not
proof that every arbitrary filter/large candidate can currently be routed.

## Regression evidence

`timeout 150 env CARGO_BUILD_JOBS=1 cargo test -p dracon-sync --locked bucket_runner_bridge -- --test-threads=1`
passes 23 tests with zero failures. Coverage includes:

- real Git outgoing/history/worktree dedup and historical deletion;
- lagging mirrors, unknown/new mirror full-history bound, explicit exclusion;
- potential Codeberg publication despite auto_create=false;
- actual Git measurements 799/800/801 fed into the JS runner's inclusive
  80%-of-1000 pressure decision (a pure function subprocess, not nested tests);
- missing declared-upstream tip and unreadable historical blob refusal;
- filter execution marker remains absent on unknown filter refusal;
- real worktree/index/HEAD/tip changes invalidate snapshots;
- symlink link-text sizing, untracked opt-out, read limit and integer overflow;
- deleted historical 700-byte blob replaced locally by a one-byte Git
  replacement still measures 700 (replacement objects disabled);
- missing measurement prevents publisher execution, manifest creation and
  index changes; success stub checks exact nonzero measured request budget.

The independent source reviewer identified replacement-ref undercounting and
Codeberg quota/cache undercounting; both were fixed and regression-tested.
Final source-only recheck reports no blockers; execution remained parent-run.
Full Node runner suite also passes (157 tests). Durable logs are stored in
`evidence/bucket-outgoing-accounting-2026-10-18/`.

This is source/test qualification only. Final workspace gates, production
artifact installation, actual OVH upload/readback/restore and live retry
remain subsequent queued items; no live-storage claim is made here.
