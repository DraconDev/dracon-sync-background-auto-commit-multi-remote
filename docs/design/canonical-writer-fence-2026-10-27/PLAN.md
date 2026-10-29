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

Current controls: 5 primitive + 4 write-fence CLI tests and the bounded
Warden resmudge apply fence-wait test pass. A new end-to-end control runs
`resmudge --apply` against a disposable encrypted tracked `.gitignore`: it
cancels while the command waits behind a held fence and checks unchanged
worktree/index bytes, then succeeds, checks plaintext worktree plus unchanged
encrypted index, and reacquires the fence after process exit. Five sequential
runs pass on dracon-warden `88d218482eeb`. The Warden/security package suite now
passes (481 passed, 0 failed, 6 ignored); strict package/all-target Clippy and
workspace `cargo deny check` pass. The first arbitrary fixture did not match a
scanner rule and produced no marker; that fixture-precondition failure is
retained separately from the passing results. Raw logs and the updated machine
receipt are under this directory. These controls do NOT establish all-writer
coverage, an overlapping cooperative writer at the exact final-close interval,
or canonical publication. No binary/settings deployment in this phase.
Task 4.1 remains open for the changed source assembly.

On dracon-sync source `aba148619237`, `cargo build --locked -p dracon-sync`
and strict all-target package Clippy now pass; targeted production hygiene and
excluded-recovery regressions also pass. The full serial `cargo test -p
dracon-sync` attempt timed out at 1800 seconds without a final suite result;
its last alert-threshold test passed separately. Retained failures, pass logs and
the bounded assessment are recorded in
`../../../evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/current-sync-controls-2026-10-29.json`.
This is not a release build or full-workspace gate.

## Integration inventory / remaining implementation

1. Warden harden_repo and repair writers: source integrated, NOT deployed.
   harden_repo uses the shared fence for managed .gitignore/.gitattributes
   writes; resmudge_repo now acquires it before inspecting/applying tracked
   canonical metadata. Generic library restore_file/migrate_markers_in_path
   reject direct canonical metadata targets. Their shared file writer, plus
   write_tracked_repair_file used by scrub/backfill/resmudge, opens without
   truncation and refuses symlink/hardlink aliases before mutation. The
   Warden/security package suite passes (481 passed, 0 failed, 6 ignored), with
   strict package/all-target Clippy on Warden `88d218482eeb`. The receipt is
   `warden-writer-controls-2026-10-28.json`. In addition to bounded apply-path
   refusal, the end-to-end tracked `.gitignore` control covers cancellation
   while waiting, successful encrypted resmudge, unchanged staged
   ciphertext/index, plaintext worktree, and fence reacquisition after process
   exit (five sequential passes; raw log adjacent). It does not test a competing
   writer at the exact final-close interval or the `.gitattributes` variant;
   there is no loaded-runtime proof.
2. dracon-sync generated-ignore/excluded-restore publisher: source integrated,
   NOT deployed. Both entry points acquire before canonical target lookup and
   capture; the existing owned helper inherits the held OFD directly, preserving
   its immediate-parent/death binding. Verification occurs before helper launch,
   exchange and settlement. Old recovery is not replayed. Fresh controls pass:
   five fence tests (including outer cancellation and actual child-held FD after
   the parent handle drops) and four generated-ignore controls (pre-open wait,
   completed writer preservation/recovery, active-writer refusal, link safety
   and stale-render refusal). Strict package/all-target Clippy passes. Receipts:
   daemon-fence-controls.json and daemon-integration-controls.json. The actual
   helper syscall positive receipt `daemon-cooperative-syscall-positive-2026-10-28.json`
   shows a Warden writer waiting at the helper-held fence, then opening and
   writing the canonical inode while the independent already-open canonical FD
   remains attached. The raw uncooperative opener control still demonstrates a
   displaced write and is intentionally a negative control, not a safety pass.
   These are bounded component controls, NOT all-writer or final runtime
   qualification.
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
   The legacy helper-process cancellation test synchronizes at the real
   pre-publication ownership lookup after private scratch is fully written,
   fsynced and closed. While A remains fenced, a direct Warden participant
   writes in independent repo B; after A is cancelled and scratch/fence cleanup
   is verified, a full Pi metadata write in B succeeds. This does NOT show a
   full Pi metadata transaction in B completing while A is held. Separate
   in-process ordinary-write cancellation controls establish cross-repo
   progress within that Pi process. Neither proves detached descendants or
   parent-only SIGKILL schedules.
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
4. Pi bash/user-bash: source integration is now present in
   `pi-plugins/extensions/pi-canonical-write-fence/index.js` and `shell.js`.
   Both the model-facing `bash` tool and `user_bash` handler use one Warden
   process-family fence for the repository owning the tool's initial cwd; the
   host Pi shell configuration is resolved only at command execution, after
   runtime binding. No command-text heuristic is used. Targeted controls cover
   actual-loader registration, same-worktree native-Git serialization,
   independent-worktree shell progress, cancellation/reacquisition, refusal
   outside a worktree, and nested descriptor propagation. The loader fixture
   supplies a settings getter stub; this is NOT an installed/live Pi runtime
   observation. The extension remains source-only and is NOT deployed.
   Critical boundary: the selected fence belongs to the initial cwd. Commands
   which `cd` or use `git -C`/absolute paths into another worktree are not
   discovered or refused by this wrapper, and detached descendants that discard
   the inherited FD are unsupported. Thus this integration does not establish
   coverage for arbitrary shell targets, native Git invoked outside the fenced
   cwd, external scripts/editors, or other live producers. No fleet producer
   inventory is complete. The latest full Pi suite attempt was 31/33; the two
   failures (registration wording and an independent-writer timeout in the
   legacy cancellation fixture) were corrected, and each affected test now
   passes alone. No post-fix full-suite pass is recorded.
5. Active-process inventory stores only PID/PPID/comm/exe/cwd, never command-line
   prompts or credentials. It identifies many live Pi sessions and Codex app
   server candidates; occurrence is NOT proof that a process writes .gitignore.
   API service names also match the broad candidate filter and are not assumed
   to be metadata producers. Static producer scan so far found platform/browser
   script matches mainly in disposable tests, plus daemon generated-ignore paths.

## Required next qualification

- Extend the Warden end-to-end schedule to `.gitattributes` and add a second
  cooperative writer contending across the actual final-close interval. The
  current `.gitignore` test checks cancellation while waiting and immediate
  fence reacquisition after the successful CLI process exits; it does not
  observe an overlapping second writer during the apply operation.
- Concurrent cooperative old/new writer schedules around actual publication;
  the current syscall positive covers one Warden participant waiting behind the
  helper. Expand it to the other supported writers and preserve the existing
  raw-uncooperative negative control. Preserve the excluded-recovery helper's
  single-finalizing-exchange/no-rollback invariant: the saved reviewer flagged
  the predecessor's second-exchange risk, while the current code returns
  unresolved on post-exchange mismatch and retains the displaced bytes. The
  current finalize-once and late-displaced-writer controls do not reproduce an
  exact simultaneous queued old/new open schedule, so do not claim general
  uncooperative-writer safety.
- Uncooperative/unknown writers remain refusal/unsupported evidence, never a pass.
- Parent death, timeout, cancellation, ENOSPC, symlink/namespace replacement,
  linked-worktree and independent-repository concurrency controls.
- All affected tool interfaces loaded and executed, not merely installed; scope
  and runtime identity receipts for each integration, without stopping producers.
- Full workspace tests/release/strict all-target Clippy/deny on one coherent
  source assembly after integrations, then qualified deployment and full-fleet
  actual publication/clean-idle observation under the original goal.
