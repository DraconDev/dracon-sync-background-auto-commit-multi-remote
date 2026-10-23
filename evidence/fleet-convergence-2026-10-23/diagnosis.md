# Fleet convergence — investigation in progress

## Scope and authorization

Confirmed goal: diagnose and repair systemic daemon/environment/hook integration
and demonstrated affected-product failures. Preserve independent publication,
finite deadlines, same-repo safety, hooks, meaningful assertions and local work.
Documented non-security staging/hygiene policy corrections and targeted retry
re-enablement after repairing causes are authorized. No manual recovery pushes,
history rewriting, force pushes, checkout deletion/rewiring or secret/trust/
ownership policy changes. The daemon remains responsible for commits/pushes.

Explicit subsequent authorization: after demonstrating that the public GOV.UK
asset name `govuk-icon-mask-<64-hex-digest>.svg` trips the OpenAI substring
matcher in unpublished history, the operator selected **Precision repair**:
correct this confirmed detector false positive with regressions preserving real
credential detection, including binary scans. No filename exemptions, allowlists,
hook bypasses or history rewrites are authorized. Inspection additionally shows
the core scanner already has an OpenAI word boundary while the generated-hook
ERE transliteration dropped it; align those paths rather than inventing a
repository-specific exception. This is the only authorized security-scope
extension; trust/ownership policies remain untouched.

Every initially affected repository must publish an observed tip autonomously;
active work is not proof of convergence. Only evidenced external blockers may
remain. This report is ongoing investigation, not a completion claim.

## Baseline

`baseline.json` records the loaded executable identity, per-repository states,
actual HEADs and configured hook paths at execution start. The loaded revision
is the previously qualified daemon, not a fresh build from this goal.
`baseline-all-repos.json` retains the complete fleet snapshot.

The initially affected set is ai-auto-video, ai-auto-writer,
pi-goal-list-loop-audit, dracon-strategy, .dracon,
browser-extensions-shared, endless-td, dracon-platform, polis and deathrun.

By this baseline ai-auto-writer, ai-auto-video, endless-td, GLLA and polis have
some successful autonomous activity; that does not establish every relevant
remote or every eligible dirty file has converged. Browser extensions remains
314 ahead; strategy 75 ahead and stuck; platform 31 ahead and blocked; deathrun
retains dirty staged content and retrying state.

## Established root causes / evidence

### Service toolchain differs from interactive shell

The running daemon's /proc environment PATH cannot resolve bun or pnpm, while
the interactive shell resolves /home/dracon/.bun/bin/bun and
/home/dracon/.npm-global/bin/pnpm. Node/npm/Git/Python/Nix do resolve in both.
The shipped service PATH also omits these package-manager executable roots.
Journal evidence includes `.githooks/pre-commit: line 24: bun: command not found`.
This is an execution-environment defect, not permission to skip the hook.

The live operator unit is a system unit, with PrivateTmp=yes. Noninteractive
sudo is unavailable. Source/runtime environment repair and the existing
user-owned binary installation path remain available; no privilege assumption
or service stop is needed to implement a durable correction.

### Gate logs exist in the daemon's private temporary namespace

The live unit uses PrivateTmp=true. Its `/tmp/dracon-pre-push.*` paths are not
host-shell `/tmp/dracon-pre-push.*`. They are readable through
`/proc/2507147/root/tmp/dracon-pre-push.*` while this daemon is running.
Therefore an absent host /tmp pathname does NOT prove the attempt log was
deleted. Earlier explanations must be rechecked against these real logs.

Current attempts UYlXi9 and mnuGZi contain per-workspace typecheck/build/test
receipts. A read-only scout is inspecting gate execution consistency and exact
product diagnostics; no product assertion changes are authorized merely to
clear a source pin.

### Other observed blocker classes (cause still to validate)

- Recent browser gate failures include vidpro-extension TYPECHECK/BUILD/TEST and
  youtube-filter TYPECHECK/TEST. These are not automatically attributed to an
  older transient source-edit race.
- Platform private-bucket preparation times out, and local commit hooks can
  reach the finite commit budget. Need process/workflow-level diagnosis.
- Polis pre-commit watchdog reports orphan bun test processes. Need verify actual
  process ancestry/identity before classifying or terminating anything.
- Deathrun pre-commit rejects 13 staged files under audit-evidence. Need reconcile
  commit-all staging with meaningful hygiene policy while retaining work on disk.
- Strategy has exhausted its retry budget after push-hook refusals. The actual
  refusal must be repaired before targeted retry re-enablement.
- GitLab SSH timeouts occur in several repos. A transient failure is not yet
  evidence of an unfixable external blocker.
- UNKNOWN scheduler state and generic 'repair --apply (push or rewrite)' hints
  need causal classification, not cosmetic relabeling or applying those hints.

## Research boundaries

The environment correction is implemented in `src/runtime_env.rs`, called by
synchronous main before Tokio creates threads. Four pure path-builder tests
pass (installed directories, inherited precedence/deduplication, absent/relative
HOME candidates, non-UTF-8 and existing empty entries). No live deployment,
service configuration, hook installation, repository index or retry-state change
has occurred in this goal; full qualification and live proof remain pending.
Thirty-two baseline gate logs were retained before any service restart, with
byte-level manifests and an explicit warning that unfinished logs are not passes.
