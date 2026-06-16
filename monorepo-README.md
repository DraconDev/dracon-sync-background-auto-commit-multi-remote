# dracon-sync

**Invisible git sync for AI-powered development.** An auto-commit, multi-mirror daemon that watches your repos, commits every change with deterministic, facts-based messages, and pushes to GitHub, GitLab, and Codeberg simultaneously.

## Why This Exists

Other tools solve parts of the problem:
- **git-auto-sync**: Auto-commits a single repo, no mirroring
- **gitea-mirror**: One-way mirror to a single Forgejo instance, no auto-commit
- **git-bridge**: Multi-provider sync, but no auto-commit or AI
- **swarf**: Invisible sync for AI agents, but only a side-band directory

**dracon-sync** is the only tool that combines all of these into one daemon:

| Capability | git-auto-sync | gitea-mirror | git-bridge | swarf | **dracon-sync** |
|---|:-:|:-:|:-:|:-:|:-:|
| Auto-commit on change | ✅ | ❌ | ❌ | ✅ | ✅ |
| Multi-repo watch | ❌ | ✅ | ✅ | ❌ | ✅ |
| Multi-provider mirror | ❌ | ✅ (→1) | ✅ | ❌ | ✅ (3+) |
| Deterministic commit messages | ❌ | ❌ | ❌ | ❌ | ✅ |
| Version bump + release | ❌ | ❌ | ❌ | ❌ | ✅ |
| Safety guards | ❌ | ❌ | ❌ | ❌ | ✅ |
| Visibility sync | ❌ | ❌ | ❌ | ❌ | ✅ |
| Broken tracking repair | ❌ | ❌ | ❌ | ❌ | ✅ |

## Features

### Invisible Infrastructure
The AI (or human) works on one repo at a time, makes changes, and sync handles the rest — the AI never needs to think about commits, pushes, or cross-repo coordination.

1. You edit files
2. Sync detects changes within seconds
3. After a brief inactivity delay (5s default), sync commits with a deterministic message
4. Pushes to origin (GitHub) and all mirror remotes (GitLab, Codeberg)
5. Done — no manual git commands needed

### HTTPS + PAT Transport (GitHub)
GitHub origin uses **HTTPS with Personal Access Tokens** — more reliable than SSH (no agent timeouts, no key rotation). GitLab and Codeberg mirrors use SSH by default, with HTTPS PAT fallback on SSH failures.

### Automatic Remote Creation
When `auto_github_private = true`, newly initialized repos without an origin remote automatically get a private GitHub repository created via `gh`, with the remote added and initial commit pushed.

### Deterministic Sync
- Monitors repositories for changes across watched roots
- Commits, pulls, and pushes automatically based on policy
- Respects freeze markers (e.g., during deployments)

### Self-Healing
- Detects and repairs common git issues (conflicted remotes, stuck pushes)
- Repairs broken upstream tracking refs (e.g. `origin/master: gone`)
- Consolidates dual main/master branch repos to main
- Manages permanently stuck repos
- Prunes stale operational state on daemon restart (stuck repos, incident ledger, visibility cache)

### Commit Messages
Deterministic facts extracted from the diff. No AI, no LLM, no prose. Routing
keys are grep-searchable via `git log --grep=`.

## Installation

### Quick Install (User Service)

Run the repository installer from the repository root:

```bash
cd dracon-utilities
./install.sh
```

This will:
1. Build the release binary
2. Install to `~/.local/bin/dracon-sync`
3. Set up and start the systemd user service

The per-utility directories do not contain standalone installers; use the root `install.sh` for all utilities.

### Manual Install

```bash
# Build
cargo build --release

# Copy binary
cp target/release/dracon-sync ~/.local/bin/

# Install systemd service
mkdir -p ~/.config/systemd/user
cp dracon-sync.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable dracon-sync.service
systemctl --user start dracon-sync.service
```

## Usage

### Commands

```bash
# Show policy path, watched roots, and discovered repos
dracon-sync status

# One-shot sync across all discovered repos
dracon-sync once

# Run continuous sync daemon (default: 1s pulse interval)
dracon-sync daemon

# Override the pulse interval from CLI
dracon-sync daemon --interval-secs 30

# Sync a specific repository now
dracon-sync sync-now ~/Dev/my-project

# Sync repos currently reported as WARN (dirty-only triage)
dracon-sync sync-now --warns
dracon-sync sync-now --warns --dry-run

# Edit the sync policy
dracon-sync config edit

# Validate the sync policy
dracon-sync config validate

# Report across all repos
dracon-sync repos
dracon-sync repos --only-concern
dracon-sync repos --json

# Repair concern repos (dry-run by default)
dracon-sync repair concerns
dracon-sync repair concerns --apply

# Repair warn repos
dracon-sync repair warns
dracon-sync repair warns --apply

# Manage stuck repos
dracon-sync repair stuck-list
dracon-sync repair stuck-unstuck ~/Dev/repo

# Manage dual-branch repos
dracon-sync repair dual-branch-list
dracon-sync repair dual-branch-repair ~/Dev/repo

# Repair orphan origin URLs (e.g. after remote rename)
dracon-sync repair origins
dracon-sync repair origins --apply

# Scaffold standard files (LICENSE, optional .github/FUNDING.yml, ...)
# FUNDING.yml is Dracon-specific; external users must opt in explicitly.
dracon-sync scaffold
dracon-sync scaffold --repo ~/Dev/repo --files LICENSE
dracon-sync scaffold --repo ~/Dev/repo --files '.github/FUNDING.yml' --dry-run

# Manually publish to registries
dracon-sync publish run ~/Dev/repo
dracon-sync publish status ~/Dev/repo

# Check daemon health and metrics
dracon-sync health
dracon-sync metrics
```

### Systemd Service Management

```bash
# Check status
systemctl --user status dracon-sync.service

# View logs
journalctl --user -u dracon-sync -f

# Restart after config changes
systemctl --user restart dracon-sync.service
```

## Configuration

Create `~/.dracon/utilities/sync/dracon-sync.toml`:

```toml
[sync]
# Watch directories for git repositories
watch_roots = ["/home/user/Dev", "/home/user/work"]

# Pulse interval in seconds (how often to scan for changes)
pulse_interval_secs = 1

# Delay after last change before auto-push (seconds)
inactivity_push_delay_secs = 5

# Auto git operations
auto_commit = true
auto_pull = true
auto_push = true
auto_bump_versions = true

# Auto-repair concerns and warnings
auto_repair_concerns = true
auto_repair_warns = true

# Automatic private GitHub remote creation
# When a repo has no origin remote, creates a private GitHub repo,
# adds the HTTPS remote, and pushes the initial commit.
auto_github_private = true
auto_github_private_account = "YourOrgOrUsername"

# Exclude specific repos or directories
exclude_repos = ["/home/user/Dev/archived"]
exclude_dir_names = ["node_modules", "target", ".venv"]

# Sync freeze (for use with dracon-system disk guard)
freeze_sync_at_action = true
```

### Automatic Remote Creation

If `auto_github_private = true` in your policy, any `git init` in a watched root will automatically:

1. Create a private GitHub repo via `gh repo create --private`
2. Add the HTTPS remote: `git remote add origin https://github.com/account/repo.git`
3. Push the initial commit: `git push -u origin HEAD`

Requirements:
- `gh` CLI must be installed and authenticated (`gh auth login`)
- `auto_github_private_account` must match your GitHub username or org

### Multi-Provider Mirrors

Push to multiple providers simultaneously. GitHub uses HTTPS + PAT; others use SSH with HTTPS fallback:

```toml
[[remotes]]
name = "github"
push_url = "https://github.com/DraconDev/{repo}.git"

[[remotes]]
name = "gitlab"
push_url = "git@gitlab.com:dracondev/{repo}.git"
auto_create = true

[[remotes]]
name = "codeberg"
push_url = "git@codeberg.org:dracondev/{repo}.git"
auto_create = false  # Codeberg/Forgejo doesn't support push-to-create
```

Store PATs for HTTPS fallback and API operations:
```bash
# GitLab
echo "GITLAB_TOKEN=<token>" > ~/.dracon/utilities/sync/secrets/gitlab.env

# Codeberg
echo "CODEBERG_TOKEN=<token>" > ~/.dracon/utilities/sync/secrets/codeberg.env
```

## Commit Messages

Commit messages are deterministic facts extracted from the diff — no AI, no inference.

### Format

```
[INTENT] | N file(s) in DIRS [files] DELTA:+A/-B [METRICS]
```

- **INTENT** — task state transitions from markdown (`CLOSED: task1, task2` or `WIP: task1`), capped at 10 tasks
- **DIRS** — top-level directories touched (root files show no `in` clause)
- **DELTA** — lines added/removed
- **METRICS** — `TEST:`, `NEW:`, `DEL:`, `DEPS:`, `BIN:`, `TESTONLY:`, `ENV:`, `TAG:`, `MERGE:`, `REVERT:`

### Why Mechanical Messages?

LLM-scribed commit messages were removed — they hallucinated context and the AI reads the diff anyway. Mechanical facts are searchable (`git log --grep="JWT"`), honest, and compact.

## Startup Cleanup

On daemon start/restart, sync prunes stale operational state:

- **Stuck repos**: Removes entries from stuck-push tracking for repos no longer stuck
- **Incident ledger**: Enforces retention policy (keeps last N entries per `incident_retention`)
- **Alert ledger**: Keeps a JSONL history of desktop-sync alert attempts under `~/.local/state/dracon/dracon-sync-alerts.jsonl`
- **Visibility cache**: Removes orphan `.last` files for repos no longer watched
- **Broken tracking**: Repairs `origin/master: gone` refs → re-points to `origin/{branch}`
- **Stale index.lock**: Removes `.git/index.lock` files with no holding process (left by crashed git operations). Without this, a stale lock blocks all git operations in that repo.

Broken tracking repair also runs every ~300 cycles (~5 min) in the daemon loop, since new `:gone` tracking breaks can appear at runtime.

## Daemon Reliability

**Push timeouts:** Default `push_op_timeout_secs=60` (was 300). A hanging mirror push blocks the entire daemon — no other repos get synced until it times out. With 3 mirrors at 300s each, a single repo could block the daemon for 15 minutes. 60s per push / 120s per repo keeps the daemon responsive.

**Filter-only cooldown:** Repos with clean/smudge filter changes (e.g. dracon-warden encryption) show as dirty in `git status` but have no diff after staging. The daemon detects this, resets the staging area, and applies a cooldown to prevent tight re-check loops.

**Fingerprint-based scheduling:** The daemon uses a fingerprint (branch + effective_dirty + staged + ahead + behind) to determine if a repo needs syncing. Only after the fingerprint stays stable for `inactivity_push_delay_secs` (default 5s) does the daemon attempt a sync.

## Push Failure Decision Tree

When a push fails, dracon-sync follows this decision tree to recover:

```
Push Attempt
├── SSH push with hardening (ConnectTimeout=10, ConnectionAttempts=2)
│   ├── Success → Done ✅
│   └── Failure → Continue
├── Retry loop (configurable retries, linear backoff 1-5s)
│   ├── Success → Done ✅
│   └── Failure → Continue
├── Transport fallback (SSH → HTTPS)
│   ├── GitHub HTTPS (no token needed for public repos)
│   ├── GitLab HTTPS (requires GITLAB_TOKEN)
│   ├── Codeberg HTTPS (requires CODEBERG_TOKEN)
│   ├── Success → Done ✅
│   └── All fail → Continue
└── Final failure handling
    ├── Diverged (ahead > 0 AND behind > 0) → Mark as stuck, skip
    ├── Clean + ahead > 0 + 3 failures → Mark as stuck, skip
    └── Other → Log incident, continue to next repo
```

### Failure Modes and Recovery

| Failure Type | Cause | Recovery |
|--------------|-------|----------|
| **SSH timeout** | Network issue, SSH agent not running | HTTPS fallback, retry |
| **SSH auth failed** | Expired key, permission denied | HTTPS fallback with PAT |
| **HTTPS auth failed** | Expired/missing PAT | Check token in secrets/ |
| **Non-fast-forward** | Remote has diverged | Merge/pull or manual resolution; stuck tracking prevents repeated failed pushes |
| **Rejected** | Branch protection, permission denied | Manual intervention needed |
| **Network unreachable** | DNS failure, firewall | Retry with backoff |
| **Timeout** | Hanging connection, large repo | Progress-aware push/pull timeout; failed operations retry or surface in `repos`/`health` |

### Stuck Push Detection

A repo is marked as "stuck" when:
- **Diverged**: `ahead > 0 AND behind > 0` — requires manual resolution
- **Clean + ahead + failures**: Repo has no uncommitted changes, has unpushed commits, and push has failed 3+ times — indicates a permanent issue (deleted remote, permission denied, etc.)

Stuck repos are tracked in `~/.local/state/dracon/dracon-sync-stuck-push-repos.json` and skipped until manually unstuck:
```bash
dracon-sync repair stuck-unstuck /path/to/repo
```

### Push Timeouts

| Setting | Default | Purpose |
|---------|---------|---------|
| `push_op_timeout_secs` | 60s | Per-push timeout for SSH or HTTPS; active pack-transfer progress extends the idle deadline |
| `pull_op_timeout_secs` | 10s | Per-pull timeout with the same progress-aware behavior |
| `push_retries` | 3 | Number of SSH retry attempts |
| `repo_sync_timeout_secs` | 120s | Compatibility/status field retained for policy visibility; per-operation progress-aware timeouts control network work |

With defaults, each network operation uses a progress-aware timeout so active transfers are not killed while idle/stalled operations are. Mirror push failures are retried and surfaced through repo reports, health checks, and incident logs.

## Report Accuracy

The `repos` command shows **real dirty file counts** from libgit2's `get_status()`, not filtered counts. The OK/WARN/CONCERN status uses `has_sync_relevant_dirty_entries()` (which excludes target/, node_modules/, oversized files, etc.), but the MOD/STG columns always show the actual number of modified/staged files. Previously, when `effective_dirty` was false (all changes excluded by policy), the report showed 0 — making repos with dozens of uncommitted files appear clean.

## Version

```bash
dracon-sync --version
```
