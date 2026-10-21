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

The full suite caught an initial lease-name regression: a `.lock` suffix
triggered the empty-repo mid-clone guard. The lease now uses the persistent
`dracon-sync.operation-lease` name, distinct from Git transaction locks.
Standalone daemon bootstrap also acquires ownership; bootstrap within a
full sync reuses its existing ownership. A read-only Git directory correctly
fails at ownership acquisition before any bootstrap mutation.

## Deployment — 2026-10-21 (the work above was committed but never shipped)

The changes in this document were committed to `dracon-sync` but were not
running anywhere: the live daemon still executed the deleted Oct-19 inode
(`sha256 2fc714a1…`, PID 2039261) and the installed CLI was the Oct-20 22:24
build (`sha256 df7ec128…`). Neither carried the operation lease, the ref-race
reconciliation, the one-shot cancellation or the push-budget work, so the
daemon and the CLI did not even share the same ownership rules.

Validation before shipping (nested repo, workspace root
`dracon-utilities/`): `cargo clippy --workspace --locked -- -D warnings` was
silent, `cargo test --workspace --locked` reported 2526 passed / 0 failed /
25 ignored, and `cargo build --release --locked` produced the release binary.

Deployment followed the pattern the earlier receipts in
`~/.local/state/dracon-sync/deployments/` already use:

| item | value |
|---|---|
| receipt | `~/.local/state/dracon-sync/deployments/20261021T051118Z/receipt.json` |
| rollback copy | `…/20261021T051118Z/dracon-sync.before` (the previous installed binary) |
| installed binary | `sha256 3b9d4fb03835cf08c837b304cdb8f11ed2a3b4eee4e20d39175dbabd9ceb0ef2` |
| previous binary | `sha256 df7ec12868123cd1b70156def998e19e3ef5af790886c46427a48bd9e39a3564` |
| old PID → new PID | 2039261 → 2042366 |
| method | atomic replace (sibling temp file + rename) + SIGTERM to the owned daemon; systemd `Restart=always` respawned it. No `systemctl stop`. |

The daemon was paused (`dracon-sync pause`) and confirmed free of in-flight
pushes before the binary was replaced, and resumed afterwards. The respawned
process executes the deployed file: `sha256sum /proc/2042366/exe` equals
`sha256sum ~/.local/bin/dracon-sync`, and the deployed binary contains the new
cancellation message and the `dracon-sync.operation-lease` name, so the
running daemon is the new code and not a same-version build of the old one.

The deployment was applied to CLI and daemon together on purpose: the
operation lease is only cooperative if both sides run the new code.

## Task budget — source parity fixed, live limit still pending

`TasksMax` was 512 in the shipped unit (`dracon-sync.service`) and in the
parent flake's `needSync "TasksMax" "512"` parity check, but the NixOS
operator module (`scripts/dracon-sync-operator.nix`) still carried 96, and
the live root-owned unit — generated on 2026-10-19 — still enforces 96. The
module now carries 512, so a NixOS install cannot reintroduce the old
ceiling.

Nothing compared those sources, which is how the drift survived; a new
`scripts/check-task-budget-parity.sh` now compares every source that defines
the budget and reports the live unit separately, because applying a change
to the live unit needs root. `scripts/test_check_task_budget_parity.sh`
proves the check fails on a drifted source, tolerates a harmless change
(a comment) and reports live-unit drift as deployment drift rather than as a
source failure. The ready-to-apply drop-in is
`scripts/30-tasksmax.conf`.

**The live unit still enforces `TasksMax=96`.** Applying the drop-in needs
the operator's admin credential, which this run did not have:

```
sudo install -m 644 scripts/30-tasksmax.conf \
  /run/systemd/system/dracon-sync-operator.service.d/30-tasksmax.conf
sudo systemctl daemon-reload
```

Both a non-interactive `sudo` and a `pkexec` polkit request were attempted
and refused ("a password is required" / "Not authorized"). The limit takes
effect for the running service at its next start; the daemon shuts down
gracefully on SIGTERM and the unit restarts it, so no `systemctl stop` is
needed. Until then the new operation lease already reduces the concurrent
task pressure that the 96 ceiling was catching.

## Remaining items (not fixed here)

- The live root-owned unit still enforces `TasksMax=96`; applying
  `scripts/30-tasksmax.conf` needs the operator's admin credential (see
  "Task budget — source parity fixed, live limit still pending" above for
  the exact commands).
- The fleet-wide performance/swap-thrash follow-ups are tracked in
  `docs/design/performance-swap-thrash-2026-10-21.md` in the parent
  repository and were not touched here.

## Resolved during this session

Recorded here so the list above stays truthful about what is still open.

- The parent repository's own push, blocked by the warden's secret scan on
  the session transcript, was unblocked: the two quoted secret-shaped
  literals were elided in place, the two never-published commits were
  replaced by one commit carrying the elided blob, and both mirrors
  (github, gitlab) now carry `28c3e1012` with the working tree clean. No
  published history was rewritten, and no hook, pattern, receipt or
  exemption was introduced. The evidence is in the parent repository's
  design note `docs/design/session-transcript-push-unblock-2026-10-21.md`.
- `browser-extensions-shared` had one unpushed commit while GitLab was
  unavailable ("Gitaly is not available at this time" / SIGPIPE from
  `gitlab.com`) — an external outage, not a daemon defect. The remote
  recovered, the daemon pushed on its own, and the repository is now 0
  ahead / 0 behind, so it is no longer an open row.
