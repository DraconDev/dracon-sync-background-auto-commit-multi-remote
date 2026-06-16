# Dracon Sync Improvement Blueprint

## Status Legend
- [ ] Not started
- [~] In progress
- [x] Completed

---

## Design Philosophy

dracon-sync is **invisible infrastructure** for an AI coder. The AI works on one repo at a time, makes changes, and sync handles the rest.

**Core principles:**
- Sync is automatic and invisible — the AI never thinks about commits or pushes
- `project-state.md` can be maintained manually by the AI for its own working memory
- Frequent commits are a feature, not a bug — more checkpoints = better recovery
- Structured output for machines (--json) > pretty text for humans

**What sync handles:**
- Auto-commit on every change
- Auto-push with HTTPS/PAT (GitHub) + SSH (GitLab/Codeberg) with retries and fallback
- Multi-provider mirroring (GitHub + GitLab + Codeberg simultaneously)
- Deterministic commit messages from diffs (no AI)
- Freeze toggle for pausing sync during delicate operations
- Incident ledger for debugging
- Visibility sync (mirror GitHub public/private status to GitLab/Codeberg)
- Self-healing (broken tracking, stuck pushes, dual branches)
- Release pipeline (version bump, tag, publish)

**What sync doesn't need:**
- Global workspace dashboards
- Session logging (AI sessions are independent)
- Interactive prompts (AI runs non-interactively)

---

## Critical Bugs (All Fixed)

### 1. Stale `status.ahead` check after commit
- **Status:** [x] Re-fetch status after commit for accurate push decision

### 2. Activity entry removed after every sync attempt
- **Status:** [x] Only remove activity entry on successful sync, track failure count

### 3. No maximum retry limit for persistent failures
- **Status:** [x] Added MAX_FAILURES constant (5), repo is skipped after exceeding

### 4. No inter-process locking for daemon instances
- **Status:** [x] Added fs2 file locking via `acquire_daemon_lock()`

### 5. Pull/rebase failure leaves repo in undefined state
- **Status:** [x] Added conflict state detection (rebase/merge/cherry-pick), skip sync and return early

### 6. Large blob detection silently ignores failures
- **Status:** [x] Properly propagate errors, skip push on detection failure

---

## Code Quality Fixes (All Fixed)

### 7. Incorrect indent calculation in version bumping
- **Status:** [x] Fixed to use `chars().take_while(|c| c.is_whitespace())`

### 8. Silent value clamping in policy validation
- **Status:** [x] Added warning messages when values are adjusted

### 9. Non-existent watch roots silently skipped
- **Status:** [x] Added warning message for non-existent paths

### 10. Redundant proto conversion functions
- **Status:** [x] Simplified to `s.clone()` and `entries.to_vec()`

### 11. Limited lockfile detection
- **Status:** [x] Expanded to detect: Cargo.lock, package-lock.json, yarn.lock, pnpm-lock.yaml, poetry.lock, composer.lock, Gemfile.lock, go.sum

### 12. TOML/Cargo.lock parse errors silently ignored
- **Status:** [x] Added warning on parse failure

### 13. Confusing nested if structure
- **Status:** [x] Fixed indentation and braces

---

## Deprecation/Migration

### 14. git filter-branch deprecated
- **Status:** [x] Added git-filter-repo detection with fallback to filter-branch

---

## Edge Cases

### 15. Cargo.lock-only guardrail may lose previously staged content
- **Status:** [x] Check for pre-existing staged content before restoring

### 16. Untracked excluded files can't be restored
- **Status:** [x] Added `can_restore_entry()` check - only Modified/Renamed/TypeChange can be restored. Untracked files now show helpful message suggesting .gitignore

---

---

## Automatic Large File Handling

### 20. Auto-add large untracked files to .gitignore
- **Status:** [x]
- **Problem:** Large untracked files (> max_stage_file_bytes, default 50MB) were perpetually detected but never handled, causing repeated warnings
- **Fix:** Added `is_large_untracked()` and `append_to_gitignore()` in `dracon-sync/src/exclude.rs`; `sync.rs` calls the large-file handler before auto-staging. Large untracked files are now automatically added to .gitignore after the managed block
- **Priority:** High
- **Location:** `dracon-sync/src/exclude.rs:567-637, 679-696, 784-828; dracon-sync/src/sync.rs:795-805`

---

## Default Exclude Patterns

### 21. Pattern-based directory exclusion
- **Status:** [x]
- **Problem:** Temp directories (`.tmp-*`) pollute repo listings with false CONCERNs
- **Fix:** Added `.tmp-*` to default excludes, enhanced `is_excluded_dir_name()` to support glob-like patterns (`prefix*`)
- **Priority:** Medium
- **Location:** `dracon-sync/src/policy.rs:497-509`
- **Default excludes:** `target`, `node_modules`, `.cache`, `.direnv`, `.venv`, `dist`, `build`, `archives`, `.tmp-*`

---

## Test Fixes

### 17. Policy reload race
- **Problem:** Policy is reloaded every loop iteration, could cause inconsistency mid-sync
- **Fix:** Clone policy at start of each repo iteration
- **Priority:** Low
- **Status:** [x]
- **Implementation:** `run_daemon()` now clones the policy inside the repo loop at `daemon.rs` so each repo has a consistent snapshot. Test: `test_policy_clone_at_repo_iteration`.

### 18. Repo discovery race
- **Problem:** If a repo is deleted between discovery and processing, sync fails
- **Fix:** Check repo existence before processing, handle ENOENT gracefully
- **Priority:** Low
- **Status:** [x]
- **Implementation:** Added `repo.exists()` check at start of each repo iteration in `run_daemon()` and `run_once()`. Non-existent repos are skipped with cleanup of activity tracking. Tests: `test_skips_nonexistent_repo`, `test_is_repo_ready_nonexistent_path`.

### 19. Status inconsistency in CLI fallback
- **Problem:** When fallback CLI entries are used, `status.staged_files` is not recalculated
- **Fix:** Recalculate all status fields in fallback path
- **Priority:** Low
- **Status:** [x]
- **Implementation:** In `compute_diff_entries()`, when fallback CLI entries are used, `staged_paths()` is now called to recalculate `status.staged_files` from actual staged paths. Test: `test_fallback_entries_recalculate_staged_files`.

---

## Config Notes

### Example dracon-sync policy values

These are example values from the public documentation, not a requirement to copy a local machine path or policy.

- `pulse_interval_secs = 1` — OK (minimum 1)
- `inactivity_push_delay_secs = 5` — OK (minimum 1)
- `pull_op_timeout_secs = 10` — OK (minimum 5)
- `push_op_timeout_secs = 60` — progress-aware timeout for active pack transfers
- `repo_sync_timeout_secs = 120` — retained for status/compatibility; network work is controlled by per-operation progress-aware timeouts
- `push_retries = 3` — OK
- `repair_cooldown_secs = 60` — OK (minimum 1)
- `max_push_blob_bytes = 52428800` (50 MiB) — OK (below common host limits)
- `max_stage_file_bytes = 52428800` (50 MiB) — OK (large-file staging guard)
- `incident_ledger_max_lines = 10000` — OK
- `incident_ledger_max_age_days = 30` — OK

### Note on "duplicate" file limits:
The config has TWO separate size limits that happen to have the same value:
- `max_push_blob_bytes` (line 48) - Guardrail to skip push if commits contain blobs > this size
- `max_stage_file_bytes` (line 80) - Skip auto-staging files > this size during commit

These serve DIFFERENT purposes:
1. Staging limit prevents accidentally adding huge files to commits
2. Push limit prevents pushing commits that already contain huge files

They're intentionally both set to 50MB to keep things simple, but could be configured differently if needed.

---

## Deterministic Commit Protocol

dracon-sync has **no AI dependencies**. Commit messages are deterministic facts
extracted from the diff. No scribe, no
bumper, no provider calls.

### Why Frequent Commits?
- Every commit is a checkpoint that downstream tools (and humans) can recover to
- More commits = more granular routing keys for `git log --grep=`
- Commits are cheap; context is valuable

### Manual project-state.md

`.dracon/project-state.md` can be maintained manually for working memory across
sessions. Sync does not auto-generate, stage, or commit this file. To track it,
`git add` it explicitly.

### Features (compile-time)
- All features are compiled in by default

### Commit Message Format
```
chore(scope): Commit subject line

# Project State

## Current Focus
What the project is working on

## Context
Why this change is being made

## Completed
- [x] Completed work items

## In Progress
- [~] Items being worked on

## Blockers
- What's stopping progress

## Next Steps
1. Immediate next action
2. What comes after

---
category: chore
scope: scope
```

### Status
- [x] Deterministic commit messages (no AI)
- [x] Routing-key format (grep-searchable)
