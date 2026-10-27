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
3. Pi built-in edit/write: current registration runs the COMPLETE native SDK
   transaction IN PROCESS with a v1 flock held through final I/O settlement.
   The Linux N-API bridge exposes only nonblocking flock and CLOEXEC duplication;
   JS validates ownership/mode/link count/namespace and bounds acquisition waits.
   No per-operation ordinary writer helper is spawned or force-killed on abort.
   Source and actual-loader fixture integration only, NOT operator deployment.
   Earlier complete extension/SDK/package/configuration/TUI documentation and
   loader examples informed the existing native schemas and queue integration.
   Atomic metadata keeps bounded private scratch and directory-FD anchoring.
   Current controls pass, including native schema/legacy preparation/dynamic-cwd loader
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
   native reacquisition. No host disk was filled. Earlier receipt pi-io-controls.json
   hashes the prototype, C interposer, Warden and compiled host SDK before/after.
   Earlier receipts remain pi-cancellation-controls.json,
   pi-registration-controls.json and pi-worker-namespace-negative.log.
   Earlier receipt pi-alias-controls.json extended the typed boundary to EVERY
   discovered-worktree edit/write before target lookup. Existing same-repo
   metadata aliases use atomic canonical publication, preserving the alias;
   cross-repo/unowned/shared-hardlink mutations refuse. Normal regular files
   retain inode/mode/in-place semantics; external ordinary files use no-follow
   descriptor checks before truncation. Broken Git metadata never falls back.
   The cancellation control now synchronizes on actual scratch readiness and
   verifies the writer has closed its temporary FD, not a discovery-call ordinal.
   CORRECTION: the old ordinary-cancel control released the blocked write
   immediately, before the five-second escalation. Holding it 6.5 seconds
   reproduced an EMPTY file with the original full-content assertion unchanged:
   ordinary-long-abort-negative.log/json (retained as FAILED).
   Fixed the lifetime boundary in lease.js/operations.js and registration: actual
   SDK I/O settles in its own process before either queue or fence releases.
   Short AND long aborts now preserve full new content, inode and mode; another
   repo publishes in the SAME Pi process during the pending write. Legacy worker
   ordinary writes now refuse before truncation. No detached future I/O is used.
   Native peer/reentrant OFD/foreign-root refusal, bounded waits, zero waiter-FD
   leakage, namespace and readonly access controls pass. A cold-umask regression
   also FAILED (0644 instead of native 0600), then passed after respecting umask;
   pi-umask-negative.log retains the failure. Discovery/external refusals count
   truthfully; cancellation can leave completed native content, not unchanged data.
   Current receipt pi-inprocess-controls-v3.json records 29 passes, zero failures
   or skips, unchanged source/SDK/Node/binding/header/native-peer inputs. Native
   build receipt: pi-inprocess-build.log. Binding is qualification-only, not
   installed. Ordinary in-process input is not helper-JSON capped; metadata's
   byte bound remains. Remaining: parent/forced-death and other I/O/deadline
   schedules (kernel I/O that never returns is NOT bounded by this proof),
   large inputs, other producers, ACL/xattr/durability, linked worktrees/devices,
   full assembly and live loaded proof. See README; no scope or gate is waived.
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
