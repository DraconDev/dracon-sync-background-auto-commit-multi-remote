# Staged epochs: integration plan and non-negotiable boundaries

Status: implementation plan, NOT implemented acceptance or deployment evidence.
The observed/captured-index and native mktree building blocks are tested and
independently reviewed. The old native commit path is still in production source.

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
