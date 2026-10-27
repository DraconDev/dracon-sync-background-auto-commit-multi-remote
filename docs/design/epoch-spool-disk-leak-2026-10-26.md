# Epoch spool disk exhaustion — repair and safety correction

## Demonstrated failure

On 2026-10-26 root had 1.6 GiB available. browser-extensions-shared's Git directory measured 115 GiB, including 141 dracon-staging directories measured together at 101 GiB. The daemon journal recorded ENOSPC during private index creation across several repositories. The staging implementation used TempDir::keep(), wrote an index and independent candidate pack, and had no deterministic successful-epoch release.

## Parent-tree exclusion: the full-copy root cause

A retained browser candidate contained 10,098 objects; 8,812 matched current HEAD blobs, totaling 1,062,761,740 uncompressed bytes. Git's tree-root traversal does not exclude the parent tree closure when the negative root is only a commit. A read-only control on the same live tree emitted 10,102 objects for `git rev-list --objects HEAD^{tree} ^HEAD` and zero for `git rev-list --objects HEAD^{tree} ^HEAD^{tree}`.

The pack input now explicitly excludes `^{parent}^{tree}`. This preserves newly staged bytes but avoids copying the unchanged baseline every cycle. A disposable regression verifies the unchanged baseline blob is absent, the new candidate blob is present, and the old commit-only exclusion reproduces the unwanted baseline copy. This is in addition to successful-epoch directory release; neither change deletes uncertain recovery data.

## Current behavior

Ordinary epochs release their own retained directory after confirmed local ref publication, including publication followed by a failed/timed-out follow-up. This does not mean remote publication succeeded. Refusals and unknown ref outcomes preserve the directory. Genuinely empty original candidates release their own directory. Empty selected candidates with rejected bytes preserve the original pack. A preserve-rejected marker prevents successful publication of the selected subset from deleting the rejected original candidate. Original conflict-index recovery is also preserved, even after the selected commit succeeds.

Age-only reaping is DISABLED. The automatic spool reaper is a no-op; repair spools --apply refuses deletion with a diagnostic. Read-only inventory remains available. No process-local lease or old mtime establishes publication or cross-process ownership.

The targeted tmp_pack sweeper also preserves files: stalled producers can retain old, open temporary packs, so mtime alone is insufficient abandonment evidence. Normal Git gc retains its normal prune grace. The stale-branch janitor's original commit-only/staged-intent guards were restored; these disk repairs must not inadvertently widen remote-ref deletion authority.

## Earlier execution and limitation

Earlier iterations in this session used age-only deletion: repair spools --apply reported 1819 directories / 83.30 GiB removed, followed by additional shorter-age backlog sweeps. A loaded tmp_pack sweeper subsequently reported deletion of 23 browser temporary packs / 10.72 GiB. Those operations were not backed by per-object publication proof or cross-process exclusion. Their apparent disk benefit does NOT prove preservation, and the session cannot claim that no recovery-only content was lost. The current safety correction prevents repeating those operations; it cannot retroactively verify removed data.

Earlier statements that the leak was fully bounded or all Git directories were stable were too broad. Small observations established successful-path release, not indefinite bounded recovery retention. Failed epochs may still accumulate preserved directories; a durable content-addressed/reachability-qualified recovery retention design remains work, not a waived requirement. Repeated temporary-pack creation also needs producer cancellation/failure investigation rather than aggressive deletion.

## Verification of safety correction and tree-delta repair

Full workspace tests, release build, strict workspace/all-target Clippy and cargo deny passed for the final tree-delta assembly. Raw logs are in `evidence/epoch-spool-safety-2026-10-26/final-tree-{tests,build,clippy,deny}.log`. The earlier 1200-second workspace timeout remains recorded separately; its isolated Warden test passed and the bounded 1800-second full retry passed without changing gates.

Behavioral boundary tests cover successful subset publication preserving rejected pack payloads, all-rejected empty selection, ordinary empty-original release, retained-spool publication followed by post-commit timeout, and real conflict-recovery publication followed by cold import of every original conflict payload. The tree-delta regression includes the former commit-only exclusion as a disposable negative control.

`final-tree-deployment.json` records the loaded executable digest matching the release artifact. `final-tree-runtime.json` records a bounded observation with no new browser candidate pack or commit observed in that window; it is NOT a successful browser runtime proof. Follow-up browser reporting still shows push-stuck state and scheduler work in progress. Full fleet-idle acceptance and the separate canonical-writer-preservation qualification remain open. Earlier weaker deployment statements do not override those requirements.

After deploying the correction, root measured 198 GiB available (78% used), and browser-extensions-shared's Git directory measured 5.0 GiB. These are point-in-time measurements, not proof of permanent bounded growth or remote convergence.
