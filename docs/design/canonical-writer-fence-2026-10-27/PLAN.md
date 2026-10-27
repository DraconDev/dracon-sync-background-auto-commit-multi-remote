# Authorized cooperative canonical-writer integration

## Authority and unchanged requirement

Operator approved integrating affected .gitignore-writing editors/agents with
one bounded write fence, acquired BEFORE path lookup/open and held THROUGH the
final close. All existing safety guarantees remain. This does not authorize a
daemon-only lock, stopping producers, blanket exclusions, accepting displaced
writers, or applying the pending count-cap migration before qualification.
The original raw-writer syscall control remains an important negative control.
A cooperative positive control must preserve canonical identity/content for
accepted writers, not invert the earlier failing assertion.

## Current implementation (NOT deployed)

Warden owns the v1 protocol: `dracon-canonical-writer-v1.lock` in the actual
per-worktree Git directory (not common-dir, not global). A persistent owned
0600 single-link regular file is flocked exclusively; it is NEVER deleted or
stolen. Acquisition has an absolute deadline, validates ownership, permissions,
namespace identity and symlink/hardlink safety. Native hardening acquires before
reading/rendering .gitignore/.gitattributes, including explicit once/repair.

`dracon-warden write-fence --repo ROOT --wait-secs 30 --run-secs 60 -- COMMAND ...`
execs a bounded writer with an inherited held descriptor. Reentrant native
writers duplicate the actual inherited OFD and verify it against the intended
repository; an env token alone is not trusted. Cross-repo token reuse refuses.
Actual writer processes must keep the descriptor through final close. The
wrapper uses timeout's owned group for ordinary deadlines and cancellation;
nested wrappers retain the outer group. Parent-only SIGKILL retains exclusion
while an actual writer still has its inherited descriptor. Full crash/cancel
family supervision remains a qualification obligation, not inferred from that
FD-lifetime test. Detached writers which discard the descriptor are NOT covered.

Current controls: 5 primitive + 4 actual CLI tests pass. Full Warden suite and
strict package/all-target Clippy pass. These do NOT establish all-writer coverage
or successful canonical publication. No binary/settings deployment in this phase.
Task 4.1 is reopened for the changed source assembly.

## Integration inventory / remaining implementation

1. Warden harden_repo: source integrated. Only production overwrite calls for
   .gitignore/.gitattributes are in that fenced render/write region. Test-only
   helpers and disposable fixture writers are not live producer integrations.
2. dracon-sync generated-ignore/excluded-restore publisher: source integrated,
   NOT deployed. Both entry points acquire before canonical target lookup and
   capture; the existing owned helper inherits the held OFD directly, preserving
   its immediate-parent/death binding. Verification occurs before helper launch,
   exchange and settlement. Old recovery is not replayed. Fresh controls pass:
   five fence tests (including outer cancellation and actual child-held FD after
   the parent handle drops) and four generated-ignore controls (pre-open wait,
   completed writer preservation/recovery, active-writer refusal, link safety
   and stale-render refusal). Strict package/all-target Clippy passes. Receipts:
   daemon-fence-controls.json and daemon-integration-controls.json. These are
   bounded component controls, NOT all-writer or final runtime qualification.
3. Pi built-in edit/write: NOT integrated. Read current complete Pi extension/SDK
   docs and checked ssh.ts replacement example. Prefer wrapping the complete
   original tool operation in an owned fenced writer worker, preserving original
   tool behavior, file-mutation queuing and abort semantics. A tool_call/result
   lease alone is insufficient if an async write outlives cancellation. Atomic
   replacement within the cooperative boundary must not leave torn canonical
   content on cancellation. Existing sessions do not magically load new extensions.
4. Agent bash/user-bash, native Git worktree mutators (checkout/merge/restore,
   etc.), scripts and other editors: coverage NOT established. Do not claim an
   extension wrapping edit/write also covers arbitrary shell commands or hidden
   dynamically constructed paths. Heuristic command matching is not proof of
   a pre-open boundary. Inventory and integrate their concrete write interfaces;
   unsupported/uncooperative writers must remain explicit negative controls.
5. Active-process inventory stores only PID/PPID/comm/exe/cwd, never command-line
   prompts or credentials. It identifies many live Pi sessions and Codex app
   server candidates; occurrence is NOT proof that a process writes .gitignore.
   API service names also match the broad candidate filter and are not assumed
   to be metadata producers. Static producer scan so far found platform/browser
   script matches mainly in disposable tests, plus daemon generated-ignore paths.

## Required next qualification

- Concurrent cooperative old/new writer schedules around actual publication;
  prove accepted canonical writes are neither displaced nor overwritten.
- Uncooperative/unknown writers remain refusal/unsupported evidence, never a pass.
- Parent death, timeout, cancellation, ENOSPC, symlink/namespace replacement,
  linked-worktree and independent-repository concurrency controls.
- All affected tool interfaces loaded and executed, not merely installed; scope
  and runtime identity receipts for each integration, without stopping producers.
- Full workspace tests/release/strict all-target Clippy/deny on one coherent
  source assembly after integrations, then qualified deployment and full-fleet
  actual publication/clean-idle observation under the original goal.
