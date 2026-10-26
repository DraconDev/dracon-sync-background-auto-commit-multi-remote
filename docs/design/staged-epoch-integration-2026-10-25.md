# Staged epochs: integration plan and non-negotiable boundaries

Status: staged-epoch implementation with intermediate assembly qualification,
NOT final acceptance or deployment evidence. The production path uses captured
private candidates; artifact cleanup and large-untracked hygiene integration
remain open. Version/release bumping was already a no-op before the epoch repair
(see preexisting-version-noop-assessment.md in the evidence checks directory),
not a disabled integration to restore here. The original transaction boundaries
below remain requirements, not an exemption. Current evidence and remaining work:
`../../evidence/fleet-idle-convergence-2026-10-25/CURRENT.md`.

## Policy-authorized stale unmerged selection

`sync/unmerged_epoch.rs` reconciles complete, verified HEAD-equal stale conflicts
before returning entry provenance. Active merge/rebase/cherry-pick states and
uncertain bytes/flags/locks/refs remain untouched. An opt-out, disabled auto-commit
or dry-run does not enable selection. A held no-follow descriptor hashes the
regular worktree source without filters, writes, relocation or killing producers.
The private candidate, not a later actual-index reread, defines the returned epoch.

Before installation, the private0700 recovery directory retains the exact original
index (including conflict stages and gitlink identity metadata) and an independent
pack of parent-repository conflict blobs and changed ordinary cached payloads.
Unchanged HEAD baseline blobs are not duplicated. Raw-index/input/payload budgets
and native blob-size/free-space checks bound the spool; free-space checking is not
a reservation against other writers. Recovery-directory and parent fsync precede
replacement, followed by index-parent fsync. No actual power-cut result is claimed.

Identity/stage/flag observation and complete source selection occur BEFORE spool
allocation: stable unknown-content or unsupported-flag retries cannot keep copying
payloads. A mixed selection with any unverifiable path refuses the whole install.
A foreign raw-index/reference change or expired deadline cannot promote a fresh
reread into provenance. No working file is restored or renamed by this operation.

The v6 coherent intermediate assembly passes all four workspace gates. Its real
sync_repo positives, meaningful hook, independent cold blob recovery, partial
staging/late replacement, live writer FD, foreign index/ref, finite cancellation
and repeated no-spool-refusal controls are in `unmerged_epoch_tests.rs`. These are
disposable Linux execution evidence, NOT installed/loaded fleet qualification.
Non-Unix descriptor reconciliation refuses conservatively; NFS/stalled-filesystem
and exact-exchange qualification remain separately open.

## Why an epoch, not a pathname allowance

Net HEAD/worktree classification can prune a pre-staged Modified path, and
re-adding that path can overwrite the distinction between staged and unstaged
bytes. A mutable on-disk index is not an immutable commit candidate. Git2.51.2
fixtures demonstrate that plain commit unlocks its index and chooses its tree
before the message hooks: a commit-msg wrapper is not an atomic final-tree gate.

An epoch must capture exact index identities BEFORE standard-file staging,
pull/merge, cleanup, preparation or other local mutation. An already-staged epoch
must not re-add its paths, update gitlinks, remove missing files, reset/restore
excluded staging or otherwise overwrite operator intent. Eligibility uses actual
cached blob sizes and all existing ownership/exclusion/private/storage policies.
Ineligible/uncertain staged content stays intact with an actionable reason.
A strict batch excess defers rather than silently unstaging or widening a cap.

## Intended separation

1. A cycle with verified cycle-entry staged intent commits only that captured
   tree. The tree helper writes unreferenced objects from captured identities,
   not live worktree data, and never changes the actual index/worktree/refs.
2. A cycle without staged intent can stage eligible work using the original
   filters/policies, but that mutation does not itself prove commit provenance.
   Close that staging epoch before capturing a fresh commit epoch. A bounded
   coordinator may perform one explicit follow-up epoch to preserve one-call
   convergence; foreign extras must retain their existing deferral behavior.
3. Never infer newly staged ownership from a read-after-add OID, authorize
   concurrent directory children by prefix, or treat matching names as matching
   payloads. Exact cycle-entry identities are known intent under commit-all's
   owned-path policy, subject to the full guards. Later unknown deltas are not
   part of the immutable candidate.

## Required commit transaction

- Verify the captured parent and explicit branch/detached identity. Root commits
  need the same protections; do not leave an unsafe bootstrap fallback.
- Resolve Git/index identity through bounded owned commands, not arbitrary
  synchronous `.git` pointer-file reads. Retain only an owned lock and preserve
  foreign/replaced locks. Do not claim Tokio timeout cancels spawn_blocking.
- Run every original meaningful commit hook exactly once, in native order,
  including message edits/refusal, expected Git arguments/editor semantics and
  post-commit only after successful publication. No --no-verify/hooksPath bypass.
- A candidate must remain policy-verified through its immutable boundary.
  Checks of a mutable index before an unprotected later write are insufficient.
  A lock-based path must account for hooks attempting to stage: never absorb or
  erase their uncertain deltas, double-run hooks, or report their refusal green.
- Keep private materialization prohibition, exact manifest/provenance/readback,
  staged-size, forward-only and explicit storage guard identity/binding checks.
  Generic repositories must not fabricate storage enrollment to reuse a seal.
  Never silently drop configured guarded storage to an ordinary commit path.
- Native commit-tree can create an immutable object without updating refs; it
  must retain configured signing/identity/message semantics. Creating an object
  is not authorization. Use native expected-old-tip ref publication so original
  reference-transaction hooks still run and a concurrent advance cannot be lost.
- Preserve the actual index/worktree on every refusal and post-publication
  follow-up. Never install an older captured index over concurrent new intent.
  A late unrelated delta must never enter the captured tree; diagnose retained
  follow-up work rather than claiming the repository idle.
- One absolute finite deadline covers the transaction and all owned descendants,
  with bounded messages, input/output/graph work. Post-commit failures cannot
  undo an already published object and must remain visible, not hidden success.

## Coordination and tests

Preserve same-repository ownership and independent cross-repository publication.
A staged-awaiting-commit result is progress/pending, not synced, clean, failure or
filter-only cooldown. Keep push-paused/backstop/failure accounting truthful and
retain activity until real publication. No new persistent policy knobs required.

Regressions must exercise partially staged/reverted worktrees plus unrelated
untracked work, filtering, deletion/rename/modes/gitlinks, exact OIDs, rejected
concurrent payloads/children, meaningful hook/ref refusals and message edits,
root/cold setup, signing refusal, strict size/batch limits, private/storage
bindings, parent advance, cancellation/process cleanup and publication isolation.
Drive the actual production path, not a parallel test-only transaction. All
previous tests/assertions and policy fields must retain their meaning.

This plan is not an exemption from any open problem. Native commit integration,
full workspace qualification, original product gates, installed execution and
all-watched clean+idle observation remain open until independently evidenced.
