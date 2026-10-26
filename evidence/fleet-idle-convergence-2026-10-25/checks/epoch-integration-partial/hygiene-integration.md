# Hygiene integration — coherent four-gate assembly, NOT deployed

Status: intermediate implementation assembly with hygiene reconciliation wired
into the production staging path. This is NOT final acceptance, NOT installed or
loaded evidence, and NOT a fleet-idle observation. Task5 remains blocked behind
those prerequisites.

## What changed

Artifact cleanup and large-file ignore handling were genuinely disabled by the
staged-epoch refactor: `Staging::finish` refused cleanup outright, and the old
callers of `remove_tracked_excluded_paths` / `handle_large_untracked` were no
longer reachable from the production path. Both are now restored through the
private-candidate model instead of the removed live-index mutators.

- `exclude/hygiene.rs` — one shared, side-effect-free renderer. Preserves all
  existing bytes (including CRLF and trailing spaces outside the block), inserts
  positive rules BEFORE Warden's protected/plaintext negations, refuses malformed
  or repeated managed markers, refuses control characters/negations/comments, and
  never rewrites an ignore file it cannot represent safely. Literal file rules are
  anchored (`/name`) and glob-escaped; artifact directory rules stay intentional
  any-depth component rules.
- `sync/hygiene.rs` — plan/detect/verify. Large untracked files above the policy
  threshold become exact ignore rules (single-link regular files only). Artifact
  paths are authorized ONLY when they are absent from the candidate and their
  captured HEAD identity is retained, and that authorization is re-verified
  against live HEAD inside the commit transaction, so a HEAD advance or policy
  change revokes it.
- `sync/hygiene_apply.rs` — bounded, no-follow confirmed observation of
  `.gitignore`, then a leased exchange through the existing recovery machinery.
  A writer holding the file keeps its inode; displaced bytes stay recoverable.
- `sync/excluded_revert.rs` — the proven apply/settle tail was extracted so
  hygiene reuses one writer. `revert_excluded_to_head` authorization is STILL
  evaluated only by the explicit restore path; generated ignore bytes never
  inherit it.
- `sync/staged_epoch.rs` / `sync.rs` — ordering matters and was corrected by
  original tests: index-side de-indexing happens in the private candidate, while
  canonical ignore rules are written only AFTER the commit landed, so new rules
  can never block the staging that produced them. An unapplied rule write is
  retained local follow-up, never a clean/idle result.

## Observed gates on this assembly

`hygiene-order-v12-*` (source identities, per-command exits and release artifact
hashes in `hygiene-order-v10-source-before.json` and
`hygiene-order-v12-source-after.json`; zero source drift, pinned helper unchanged):

- `cargo test --workspace --locked`: PASS — sync 1484 passed/0 failed/3 ignored,
  system 616, warden 233, integration 144; security and stream suites in the log.
- `cargo build --release --locked`: PASS.
- `cargo clippy --workspace --locked --all-targets -- -D warnings`: PASS.
- `cargo deny check`: PASS, no guard weakened.

Retained, unweakened: `hygiene-order-v10-workspace-tests.log` records one failure
of `excluded_recovery_parent_crash_kills_actual_lease_holder_without_late_restores`
(kernel lease `EAGAIN`) under full-suite load; the identical control passed
immediately in isolation (`hygiene-order-v11-crash-retry.log`) and again in the
v12 suite. That is a load-timing flake in the lease control, not a weakened test.

Positive controls run through the real `sync_repo` path: artifacts are de-indexed
with worktree bytes preserved, oversized files are ignored (not committed, not
deleted), ordinary operator edits still commit in the same cycle, opt-out and
dry-run leave tracking and `.gitignore` untouched, foreign operator staging is
never reconciled around, and a live writer keeps its inode.

## Still open

- Queued-open/lease-break and exact-exchange interleavings, unsupported-filesystem
  controls, actual daemon restart/admission rehearsal.
- Original product gates on repaired repositories, independent full-contract
  rehearsal.
- Installed/loaded execution receipt with matching artifact identity, and the
  authorized zero-batch live migration.
- Actual daemon publication plus a fresh complete watched-fleet observation with
  every repository clean and idle. The earlier 37-repository count is a baseline
  observation, never a fixed inventory.

No installation, service/config migration, or real-repository manual commit/push
was performed here. Do not deploy this assembly as a final release.
