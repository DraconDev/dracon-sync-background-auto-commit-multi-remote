# Sync operation ownership and durable push evidence — 2026-10-21

## Recurring failure modes

Recovery at one snapshot did not guarantee later edits could publish. The
browser repository continued receiving changes, and later gate refusals
were genuine new failures. Fixed log names meant a later passing run could
overwrite the failed attempt's evidence. Manual sync-now and daemon workers
could also race: an already-published desired tip generated a ref-update
rejection and burned another retry. Separately, a hard 900-second worker
watchdog undercut a scaled 1800-second push timeout.

## Changes

1. Every mutating sync_repo call takes a nonblocking kernel file lock in the
   shared Git directory. CLI, daemon, aliases and linked worktrees share it.
   A busy lock defers work without recording a failed push. The file stays
   in place; dropping the descriptor or exiting releases ownership, so no
   stale-lock deletion is needed. Dry-run stays read-only. Symlink and
   hard-linked lease files fail closed. This is cooperative ownership for
   sync commands; direct operator Git remains possible.
2. Worker watchdogs receive a per-operation budget derived from configured
   local work, scaled push timeouts, target count, retries and the command
   runner's progress hard limit. Queue-only reservations still do not consume
   execution time, and cancellation still retains ownership until teardown
   joins. The deadline remains finite; it no longer blindly fires at 15
   minutes during an otherwise permitted multi-remote operation.
3. Ref-update races are reconciled only with fresh evidence: local HEAD must
   stay unchanged while ls-remote proves that exact tip on the exact branch.
   Missing/unreachable/divergent remote evidence fails closed. Hook refusals
   never take this path. Reads have bounded timeouts and owned process-group
   cleanup. Both origin and named-mirror paths use the same decision.
4. One-shot sync-now and once handle TERM/INT by cancelling their owned
   operations before exiting. This releases the repository lease and tears
   down Git/hook process groups instead of orphaning them. A process-level
   regression verifies both child cleanup and immediate lease reacquisition.
5. The browser pre-push hook uses private per-attempt directories and unique
   changed-file lists. Concurrent runs cannot clobber each other's scope or
   diagnostics. A failed gate reports its exact exit status and evidence path;
   all four gates and secret scanning remain enabled. General tmp retention
   handles old attempt directories; failed evidence is not removed at exit.

## Validation

Regression coverage includes real cross-process lease exclusion and release,
linked-worktree ownership, unsafe lease-file refusal, derived watchdog expiry,
positive and negative ref-race decisions against real local remote refs, and
concurrent hook failures with different evidence directories and status 37.
Successful hook behavior is tested too. Workspace checks and deployment
evidence are recorded after completion.

## Deployment discipline

Both CLI and daemon must run the new code to share ownership. Replacing only
the CLI leaves an old daemon unaware of the lease. Drain in-flight work under
automatic-resume maintenance before replacing the binary; preserve the old
installed CLI and the separately loaded daemon executable as rollback copies.
The system service has Restart=always with a five-second restart delay. Never
stop the service; verify automatic restart and the loaded executable hash.

One old in-flight hook encountered a partial read while its tracked source
was being edited during this investigation. The final file passes shell syntax
and hook regression tests. Further hook updates must be installed atomically,
and the final retry follows drain/verification rather than editing a running
script. No hook bypass or manually fabricated success observation is allowed.

The root-owned system service still has TasksMax=96. Package defaults are
already prepared for 512; applying the live change requires administrator
authentication. The code changes reduce overlapping work and preserve accurate
failure evidence; they do not silently claim that root setting was applied.

## Concurrent source edits remain visible

At 03:19 BST the retained attempt `dracon-pre-push.aSixZW` identified a
real TabMon TYPECHECK refusal: BackupSection.svelte ended mid-input while
another agent was editing it. The file was complete on subsequent inspection.
These ownership changes serialize sync operations, not arbitrary editor writes.
A future gate design should validate a stable pushed-tree snapshot; it must
account for generated inputs and shared dependencies without increasing disk
or memory pressure. Until then, gate failures remain failures and must be
checked against the retained attempt rather than hidden by retries.
