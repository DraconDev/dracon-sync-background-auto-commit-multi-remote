# Current continuation: publication-completion deadlock

The goal remains open. **Current attempt is v8**: v7 was rejected after a real
debug-stack overflow and strict-Clippy unused-field failure. Its timed-out
executor's release status was unknown, not a pass; see v7/interruption.json.
Do not treat v6's nine-of-ten ledger or local product checks as final replacement
execution/publication proof.

## Proven defect and repair

Captured loaded source `fa9728b7358372c1fc169ca776fee551b9057ae7` restored the
outer local semaphore permit inside `push_phase(upload)` before the actor joined
an already queued refresh. With one permit, a healthy unrelated local worker can
hold capacity while restoration queues first and refresh queues second. On release,
restoration owns capacity and awaits the refresh that needs that capacity.

This is a source-proven ordering defect, reproduced by the extracted production
coordinator (red timeout). It matches, but is not a live-stack attestation of,
the observed old runtime: twelve aged Working reservations, no child commands,
free operation leases, held publication leases, null publication stamps, and a
tracked edit remaining uncommitted after the separate maintenance released.

`push_resources::publication_round` now keeps upload AND pending-refresh drainage
inside the yielded-capacity boundary. `run_publication_actor` directly uses it.
Actual mutation is not cancelled; parent capacity returns before post-upload
mutation. No reservation/lease is discarded to manufacture progress. Seven
extracted resource/coordinator tests pass, including success/error FIFO races and
started-mutation drainage. Fresh reviewer found no issue in the narrow repair.
Evidence: `checks/publication-drain-deadlock/`.

## Qualification in progress

V8 now requires the upload future to be boxed at the coordinator API boundary,
not only after its first poll. This prevents large upload frames from being
multiplied through nested async construction. A stack-budget regression was
added; the actual previously overflowing production integration case passes
with its unchanged default debug stack (`workspace-final-v8/focused-actor.log`).
The unrelated system refusal timestamp was unused; it was removed without
changing reason-only log suppression or adding a warning exception. V8 reuses
only an owned, idle private target cache. Required full gates are running with
an explicit long executor envelope; no v8 pass or deployment is claimed.

A receipt-owner cancellation control additionally entered bounded cleanup,
joined its private daemon and left no fixture processes, with the real service
unchanged. The control used the old installed binary and is not a v8 behavioral
qualification. Candidate/installed receipts still must execute the new artifact.

### Historical v7 setup (superseded by v8 capture)

`checks/workspace-final-v7/source.json` captures four canonical Git refs plus an
explicit exact worktree overlay of 255 compiled/runtime inputs. This is NOT
falsely described as Git-ref equality: the old runtime cannot commit its own
repair while wedged. No canonical index/ref/config/add/commit/push operation was
performed. Existing required smudge-filter config is unchanged. A private normal
SDK install passed; all captured input bytes are checked before/after required
gates. Private assembly/target only, outside watch roots.

Worker `06d7041a-f5e4-4d34-af34-9af505dc21b1` owns only qualification logs/receipt;
it runs bounded `qualify.py`. Last assessment: cold dependency compilation was
progressing, not a test pass. Do not duplicate or cancel merely for a 240-second
attention notice. Collect its native result and inspect actual failures.

`stage-and-receipt.py` refuses nonpassing qualification, validates overlay and
remaining ordinary-smudge inputs, stages exact bytes, and runs isolated local
transports. The extended receipt uses one local permit and additionally requires
completed A's publication lease to retire, then a NEW A edit to commit/publish.
It keeps earlier blocked-A/independent-B, same-repo edit during push, all commit
hooks, refused-C preservation/recovery and joined-cleanup assertions. Signal
handling enters bounded fixture cleanup. No live deployment has occurred for v7.

## Next authorized steps

1. Collect and assess v7 required Rust gates; repair any actual failure, with
   coherent rerun. No advisory exceptions, lifecycle bypasses or deadline changes.
2. Run bounded v7 candidate staging/receipt only after qualification passes.
3. Use the prepared qualified-deployment script ONLY via the existing maintenance
   wrapper and only after candidate proof. It requires the current loaded v6 hash
   `59ef460ef335d51aafd09f01d7537589a70712254feb2557751c440f62146cfc`, backup,
   atomic replacement, pidfd SIGTERM and verified Restart=always replacement.
   Never stop the service. Respect any separate maintenance owner.
4. Run installed-byte receipt and verify loaded executable equality. Then capture
   a NEW final-runtime baseline and eligible work for ALL ten repositories; v6's
   historic nine rows are not fresh proof for a replacement runtime.
5. Require normal daemon-owned full browser gates and all intended advertised
   descendant refs. Fresh journal/ledger, all tasks and independent review remain
   mandatory before a completion claim.

## Late product gates already repaired, not final publication

Gemini canonical defaults + imperative helper rename: current all package gates
passed, narrow review clean. Its owner's later missing `setFolderColor` handler
was already wired; fresh current typecheck/lint passed.

VidPro: historic measurement-script lint corrected (owner later rewrote/deleted
the temporary script). No production layout change. Semantic helper/cell/label
and anti-bottom guards plus harmless/negative fixtures pass; real empty-profile
browser geometry verifies separate columns/aligned edges and rejects an offset.
Current 1376 unit tests, lint/typecheck/build and browser geometry passed.

YouTube Filter: production defaults/filtering were not changed. Tests reconcile
explicitly documented operator-requested defaults, watched-surface policy and
three treatment modes. Existing stored OFF/no-op import/allow/exception guards
remain meaningful; OFF/unwatched fixtures now use an eligible subscriptions
surface. UI registry exports replace repeated checkbox-count refreshes. Current
971 tests and package gates passed; final watched-fixture subset42 passed. Late
reconciliation review is clean. Logs: `checks/late-product-gate-reconciliation/`.

Separate dracon-system release maintenance paused the old runtime19:23–19:50UTC.
It released itself; neither its owner nor marker was touched. This temporary
condition is not accepted as a final external blocker.
