# Code Context

## Result
- Current main observed at `ca8fe74af7c0ce2cfec4b6df978b365dce4e1ebe`; read-only research, not implementation or fleet acceptance.
- **Current Doomtap is NOT enrolled in either Prepared workflow.** Live `bucket-runner.json`, `bucket-assets.json`, and `.dracon/bucket-assets.json` are absent. Consequently the materialized-path partition cannot explain its current repeated extras guard. Historical enrollment during earlier log events remains unverified.
- A real source-proven independent wedge exists: classification can discard pre-staged Modified paths based on net worktree-versus-HEAD diff, while the extras guard still sees their cached changes. Existing tests do not cover this case. Evidence does NOT establish that this is the actual Doomtap instance; do not present it as confirmed root cause.

## Files Retrieved
1. `src/sync.rs` (691-744, 854-946, 1180-1208, 4370-4461, 4513-4642, 5783-5821, 5985-6115, 12266-12390): cleanup, classification, private suppression, extras guard, security gates, and current concurrency tests.
2. `src/bucket_assets.rs` (1-287, 470-558): Prepared provenance, strict materialized-path rejection, baseline-preserving integration test.
3. `src/bucket_runner_bridge.rs` (237-287, 445-463, 530-606): root-only discovery, manifest parsing, verified verdict/digest, output ceiling.
4. `src/git/diff.rs` (8-102, 304-332, 440-461): bounded filter-aware HEAD diff and independent cached-path query.
5. `src/exclude.rs` (1701-1802): worktree-size staging gate versus staged-blob-size variant.
6. `/home/dracon/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/dracon-git-94.7.2/src/lib.rs` (260-285; search hits 1034-1067): dependency status-derived DiffFile generation, M classification.
7. `docs/design/private-bucket-pilot-2026-10-14.md` (7-83): explicit eligibility, verified upload, retained tracked baselines, forbidden raw materialization staging.
8. `docs/design/bucket-routing-contract-2026-10-17.md` (1-115): routing/accounting contract; historical implementation-status paragraphs are not current authority.
9. `docs/design/filter-ceiling-accounting-2026-10-19.md` (1-58): superseding output-ceiling reservation and final staged-size enforcement.
10. `evidence/fleet-idle-convergence-2026-10-25/doomtap-problem-path.json` (1-14); `baseline-repos.json` (parsed structured document); `doomtap-index-entries.z` (binary NUL entries parsed): fresh baseline evidence.
11. `/home/dracon/Dev/dracon-platform/web/games/wip/doomtap/scripts/assets/filter-clean.py` (1-85), `.gitattributes` (13-139), `web/config/bucket-strategy.json` (parsed document): actual legacy filter path, not Prepared.

## Key Code
- `staged_extra_paths`: intent consists ONLY of `to_stage` exact paths and descendants of currently existing intended directories (`sync.rs:1196-1207`). It checks names, not blob OIDs, modes, or when an index entry appeared.
- `compute_diff_entries` obtains status-derived entries, then retains Modified entries only if `git_diff_head_files` contains their path (`sync.rs:854-887`). That query is `git diff HEAD --name-only -z` (`git/diff.rs:81-102`).
- Cached/CLI fallback runs only when the entire remaining entry list is empty and not filter-only (`sync.rs:898-940`). One unrelated new/dirty entry prevents recovery of individually lost staged candidates.
- `sync.rs:6000-6006` removes Prepared materializations before partitioning; `4396` suppresses them again during expansion. Neither removes existing index entries.
- `clean_staged_paths` only removes excluded/oversized staged entries and optionally tracked artifact paths (`691-744`); it does not reconcile enrolled materializations or reconstruct staged intent.
- Extras cause whole-pass Blocked with index retained (`4439-4455`). The comment promising next-cycle convergence assumes classification includes every pre-staged path; this is not guaranteed.
- Even blanket-authorizing private materialized extras would then fail `Prepared::verify_staged` (`bucket_assets.rs:250-282`): exact verified manifest required, every changed materialized/cache path forbidden. Bypassing either gate would violate the storage contract.

## Architecture / Actual Evidence
- Prepare precedes diff, then partition, staging, hygiene, cached-name extras gate, ownership/storage/hooks/sealed-or-native commit. Prepared is optional and fail-closed on preparation failure, not silently ignored.
- Legacy prepare verifies upload verdict plus exact manifest SHA256 before constructing suppression membership. Bridge requires commitEligible verdict, digest equality, safe manifest sources, and staged-output ceiling; constructor alone is NOT proof of per-path current worktree/index payload identity.
- Baseline: 37 watched repos, 30 OK / 6 active / 1 warn. Doomtap: 120 staged, 221 untracked, zero ahead, retrying; baseline does not satisfy all-37 clean+idle acceptance.
- Problem path baseline and current index: mode 100644, OID `bbb838bc04782b4fe46143895717ee5cec6f9e56`, cached Modified. Current HEAD OID `d7c00200ab85edae223558eb7d93e6ac6b02bfae`.
- Both Git blobs are **213-byte legacy pointers**, not 1.7 MB PNG blobs. HEAD pointer SHA256 `868eb0b2f653f7c1d1f5c8ba00f978f3993a8de3658feaa5eed4b41f3153b474`; index pointer SHA256 `2da8cf3ce113c14e86d375354c68e483f2a20158ea3e3a09509b40da8b656306`.
- Current raw worktree: 1,732,735 bytes; SHA256 exactly equals index pointer payload SHA256. Index points to `quarantine/doomtap/<sha>.png`. This is not proof of remote readability or provenance qualification.
- Effective attribute: `filter=dracon-assets`, `diff=unset`. Local clean command invokes `scripts/assets/filter-clean.py`; required=false. Later attribute lines override earlier PNG filter exclusions. A staging inclusion such as `!*.png` does not disable this clean transformation.
- Clean script may upload and mutate its manifest, preserves pinned pointer for matching payload, and passes raw bytes through on missing credentials/storage errors. Therefore I did NOT run live status/diff HEAD/add/clean filters to reproduce the classification output. Read-only Git queries used optional locks disabled and cached/tree/blob/attribute/config inspection only.

## Maintainable Fix Direction (not implementation)
1. Separate **cycle-entry staged intent** from **worktree staging candidates**. Capture exact cached entry identity (path/mode/OID/stage plus HEAD/index identity) before classification/staging. Do not infer staged intent solely from net HEAD/worktree comparison.
2. Individually merge eligible initial staged candidates into commit intent even when unrelated entries survive classification; retain filter-only handling for worktree-only noise. Apply policy and measured staged-blob ceilings, not blanket consent to all cached paths.
3. Do not re-add initial staged paths indiscriminately: a partially staged file's unstaged bytes are distinct intent. Preserve index content and worktree content separately; unknown same-path concurrent replacements must defer, not count as previously authorized by matching pathname.
4. Retain foreign-extra deferral. Bind accepted staged entries to snapshot identities; directory intent must not authorize arbitrary concurrent child additions. Revalidate before the immutable commit boundary, including hook changes. Avoid a pathname-only second-check patch that leaves same-path races open.
5. For genuinely enrolled materializations, keep strict raw-blob prohibition and retained HEAD baseline. A future reconciliation needs verified per-path payload/index provenance plus conditional index edits preserving concurrent changes; current Prepared membership alone cannot authorize unstaging/replacing operator intent. Escrow/journal or alternate-index transaction semantics require design review, not an ad hoc reset.
6. Keep ownership, forward-only, hook lifecycle, staged-size ceilings, upload/read-back, exact manifest and sealed expected-old-tip guards. Do not disable filters, increase caps, remove enrollment, hide files, stop producers, or reset the index to force green.
7. Finite absolute deadline and object/path/output caps across snapshot, classification, identity revalidation and any retry. Surface durable reason codes for absent-from-diff / policy-rejected / private-materialization / concurrent-identity-change instead of endless generic concurrent-staging messages.

## Specific Regressions / Verification Gaps
- Add real-repo test: staged tracked change, worktree reverted to HEAD, unrelated untracked/dirty path; cached intent survives without requiring empty-list fallback.
- Add filter-aware counterpart where canonical worktree output equals HEAD but cached staged blob differs; preserve staged payload and unstaged bytes, do not misclassify FilterOnly.
- Add existing test's second-cycle case with initial staged M, partially staged content, rename/delete/typechange, and directory child extras; assert exact committed OIDs and untouched worktree bytes.
- Add same-path index OID/mode replacement and HEAD advance after capture/before commit (including hooks); uncertain writes remain intact and defer under finite timeout.
- Extend private baseline test with pre-staged enrolled materialization, concurrent replacement, manifest drift and storage outage; no raw materialization commit, no erased operator bytes/index intent. Existing test explicitly expects raw materialization to remain modified against old Git baseline (`bucket_assets.rs:531-536`).
- Actual Doomtap requires bounded observation of classification candidate/path-rejection reasons from the running binary, with source/build/config identity and timestamps. Supplied cached/path evidence does not record the failing pass's `diff_output`, `entries`, or `to_stage`; no filter subprocess was executed here.
- Preserve original concurrency tests at `sync.rs:12270-12390`; they only prove ordinary extra.txt next-cycle convergence, not filters/private membership/same-path payload races.

## Start Here
Open `src/sync.rs:854-946` first: staged intent is currently lost upstream of the extras guard. Then inspect `1191-1207`, `4439-4455`, and `6000-6006`; do not weaken the guard before identifying which upstream omission occurred in the actual daemon pass.

## BLOCKERS
- Actual Doomtap recurring guard cause remains unproven; current private enrollment hypothesis is contradicted by absent config. Net-HEAD pruning is a demonstrated source seam, not confirmed live diagnosis.
- Strict all-37 Git-clean acceptance can conflict with the explicitly tested enrolled-baseline contract, which intentionally leaves materialized worktree modifications. Report this conflict; do not silently redefine clean as publication-complete or discard/ignore materializations.
- No all-37 final observation, runtime deployment verification, remote payload verification, or tests performed. One broad registry-path discovery command hit its 10s inspection timeout; subsequent bounded dependency lookup succeeded. Only the requested research artifact was written; no live repo/config/index/refs modified by this research.
