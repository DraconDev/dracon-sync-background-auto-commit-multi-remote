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
3. Pi built-in edit/write: worker and registration implemented and exercised
   through the actual Pi loader in disposable fixtures; NOT deployed to live
   sessions by this work. Re-read current complete extension/SDK/package/configuration/TUI
   documentation and checked ssh.ts and SDK loader examples. Prototype reuses
   the host's complete tool operation and queue AFTER validating the actual
   inherited locked OFD and its repository. Atomic writes use private, byte/inode
   bounded scratch and an open directory anchor for publication/cleanup. Nineteen
   controls pass, including native schema/legacy preparation/dynamic-cwd loader
   contracts and actual fenced SDK BOM/CRLF/Unicode/diff semantics,
   ambiguous-edit preservation, refusal without a held fence, pre-publication
   cancellation, stale render, link safety and scratch namespace substitution.
   The namespace regression initially FAILED (foreign cleanup entry deleted),
   then passed after directory-FD anchoring; its failure remains in the log.
   An actual family-cancellation schedule pauses after private scratch fsync
   before rename. An independent repo publishes while the first fence is held;
   cancellation preserves its original canonical inode/bytes, clears scratch,
   exits the blocked child and allows native reacquisition. This is NOT proof
   for detached descendants or parent-only SIGKILL schedules.
   Native write/writev and fsync ENOSPC fault controls also pass: each refuses,
   preserves the canonical inode/content, clears owned scratch and permits
   native reacquisition. No host disk was filled. Current receipt pi-io-controls.json
   hashes the prototype, C interposer, Warden and compiled host SDK before/after.
   Earlier receipts remain pi-cancellation-controls.json,
   pi-registration-controls.json and pi-worker-namespace-negative.log.
   Latest receipt pi-alias-controls.json extends the typed boundary to EVERY
   discovered-worktree edit/write before target lookup. Existing same-repo
   metadata aliases use atomic canonical publication, preserving the alias;
   cross-repo/unowned/shared-hardlink mutations refuse. Normal regular files
   retain inode/mode/in-place semantics; external ordinary files use no-follow
   descriptor checks before truncation. Broken Git metadata never falls back.
   The cancellation control now synchronizes on actual scratch readiness and
   verifies the writer has closed its temporary FD, not a discovery-call ordinal.
   Latest ordinary-cancel receipt proves abort after open/truncate settles to
   full new bytes with the same inode/mode, reports cancelled, releases the
   fence and leaves no scratch. Aborted ordinary writes intentionally leave the
   completed new content like the native SDK; they do NOT restore the original.
   Remaining blockers: SIGKILL mid-write, timeout kills, parent-death, large
   inputs, other producers, ACL/xattr, full assembly and live loaded proof.
   Existing sessions do not magically load source. See README limitations.
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
