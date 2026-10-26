# Epoch spool disk exhaustion — repair and safety correction

## Demonstrated failure

On 2026-10-26 root had 1.6 GiB available. browser-extensions-shared's Git directory measured 115 GiB, including 141 dracon-staging directories measured together at 101 GiB. The daemon journal recorded ENOSPC during private index creation across several repositories. The staging implementation used TempDir::keep(), wrote an index and independent candidate pack, and had no deterministic successful-epoch release.

## Current behavior

Ordinary epochs release their own retained directory after confirmed local ref publication, including publication followed by a failed/timed-out follow-up. This does not mean remote publication succeeded. Refusals and unknown ref outcomes preserve the directory. Empty selected candidates preserve their original pack because it can contain rejected bytes. A preserve-rejected marker prevents successful publication of the selected subset from deleting the rejected original candidate. Original conflict-index recovery is also preserved, even after the selected commit succeeds.

Age-only reaping is DISABLED. The automatic spool reaper is a no-op; repair spools --apply refuses deletion with a diagnostic. Read-only inventory remains available. No process-local lease or old mtime establishes publication or cross-process ownership.

The targeted tmp_pack sweeper also preserves files: stalled producers can retain old, open temporary packs, so mtime alone is insufficient abandonment evidence. Normal Git gc retains its normal prune grace. The stale-branch janitor's original commit-only/staged-intent guards were restored; these disk repairs must not inadvertently widen remote-ref deletion authority.

## Earlier execution and limitation

Earlier iterations in this session used age-only deletion: repair spools --apply reported 1819 directories / 83.30 GiB removed, followed by additional shorter-age backlog sweeps. A loaded tmp_pack sweeper subsequently reported deletion of 23 browser temporary packs / 10.72 GiB. Those operations were not backed by per-object publication proof or cross-process exclusion. Their apparent disk benefit does NOT prove preservation, and the session cannot claim that no recovery-only content was lost. The current safety correction prevents repeating those operations; it cannot retroactively verify removed data.

Earlier statements that the leak was fully bounded or all Git directories were stable were too broad. Small observations established successful-path release, not indefinite bounded recovery retention. Failed epochs may still accumulate preserved directories; a durable content-addressed/reachability-qualified recovery retention design remains work, not a waived requirement. Repeated temporary-pack creation also needs producer cancellation/failure investigation rather than aggressive deletion.

## Verification of safety correction

Bounded cargo clippy -p dracon-sync --locked --all-targets -- -D warnings passed. Focused tests passed for successful commit spool release and unpublished retention, original-conflict/rejected-content preservation, age-only apply refusal, and temporary-pack preservation. Release build passed. Deployment receipt at ~/.local/state/dracon-sync/deployments records label spool-safety-preserve-unpublished and verifies the loaded process executable digest equals the built artifact. Full workspace qualification and fleet-idle acceptance are still outstanding for this corrected assembly.

After deploying the correction, root measured 198 GiB available (78% used), and browser-extensions-shared's Git directory measured 5.0 GiB. These are point-in-time measurements, not proof of permanent bounded growth or remote convergence.
