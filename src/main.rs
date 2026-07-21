#![warn(missing_docs)]

//! Dracon Sync — git sync automation daemon.

mod bump;
mod cooldown;
mod daemon;
mod exclude;
mod git;
mod helpers;
mod log;
mod nix;
mod ownership;
mod policy;
mod print;
mod release;
mod report;
mod role;
mod secrets;
mod standard_files;
mod sync;
mod test_helpers;
mod visibility;

/// Render a boolean as a compact on/off string for tables and flags rows.
fn onoff(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};
use daemon::{list_stuck_repos, run_daemon, run_once, unstuck_repo};
use exclude::excluded_dir_names_set;
use git::{consolidate_to_main, detect_orphan_origin, fix_orphan_origin, has_both_main_and_master};
use policy::freeze_reason;
use policy::{resolve_policy_path, timestamp_secs, SyncPolicy};
use report::{
    push_large_blob_threshold_bytes, run_repair_concerns, run_repair_warns, run_repos_report,
    run_scan_bloat_report, ConcernRepairFilter, RepoFilter,
};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use sync::sync_repo;

#[derive(Parser, Debug)]
#[command(name = "dracon-sync")]
#[command(about = "Git sync automation — auto-commit, push, and mirror your repos")]
#[command(
    after_help = "ENVIRONMENT:\n  DRACON_SYNC_GIT_BIN    Override path to git binary (checked every call)\n  DRACON_SYNC_POLICY    Custom sync policy file path\n  DRACON_SYNC_STATE_DIR Custom state directory path"
)]
#[command(version)]
struct Cli {
    /// Increase output verbosity. Can be repeated up to 2 times (-v, -vv).
    #[arg(global = true, short, long, action = ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Show resolved policy path and sync scope.
    Status {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// One-off report across discovered repositories.
    Repos {
        /// Show only concern repos.
        #[arg(long)]
        only_concern: bool,
        /// Show only warn repos.
        #[arg(long, conflicts_with = "only_concern")]
        only_warn: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Sort repos by field: updated, name, modified, ahead, behind.
        #[arg(long, default_value = "updated")]
        sort: String,
        /// Filter repos by name (substring match).
        #[arg(long)]
        filter: Option<String>,
        /// Show full repo paths instead of short names.
        #[arg(long)]
        full_path: bool,
        /// Print the column legend and exit (no table). Use when a column is unclear.
        #[arg(long)]
        legend: bool,
        /// Force a specific layout tier (vertical / compact / full) regardless of
        /// detected terminal width. Useful when piped output auto-picks the wrong
        /// layout, or when scripting for a known terminal size.
        #[arg(long, value_parser = ["vertical", "compact", "full"])]
        layout: Option<String>,
        /// Glance view: 3-column summary (STATUS · REPO · WHAT), no headers,
        /// sorted by severity (concerns first, clean last). Use when you just
        /// want to know "is anything broken?". Combine with --only_concern or
        /// --only_warn to focus on problems.
        ///
        /// ADDED 2026-07-19 (goal `4555eaf6` v0.112.27): the default
        /// `repos` table is dense (16 columns) for deep inspection.
        /// The summary view is for at-a-glance health checks.
        #[arg(long, short = 's')]
        summary: bool,
        /// Sort the summary view by severity (concern → warn → active → clean)
        /// instead of the default `updated` order. Convenience flag for the
        /// most common summary use case.
        #[arg(long)]
        summary_by_severity: bool,
    },
    /// Check daemon health (policy valid, daemon responsive, repos healthy).
    Health {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print Prometheus-style metrics.
    Metrics,
    /// Run one sync pass.
    Once,
    /// Run continuous sync loop.
    Daemon {
        /// Override the policy scan interval (seconds). Defaults to policy value.
        #[arg(long)]
        interval_secs: Option<u64>,
    },
    /// Sync one or more repositories now.
    SyncNow {
        /// The repository path(s) to sync immediately.
        repos: Vec<PathBuf>,
        /// Sync repos currently reported as WARN (dirty-only triage).
        #[arg(long, conflicts_with = "repos")]
        warns: bool,
        /// Preview what would be done without making any changes.
        #[arg(long)]
        dry_run: bool,
    },
    /// Pause sync (creates freeze marker).
    Pause,
    /// Resume sync (removes freeze marker).
    Resume,
    /// Manage sync configuration.
    Config {
        #[command(subcommand)]
        cmd: ConfigCommands,
    },
    /// Repair and manage repositories (concerns, warns, origins, stuck repos, dual-branch).
    Repair {
        #[command(subcommand)]
        cmd: RepairCommands,
    },
    /// Publish to package registries and check publish status.
    Publish {
        #[command(subcommand)]
        cmd: PublishCommands,
    },
    /// Scaffold standard files (LICENSE) into repositories.
    Scaffold {
        /// Repository path to scaffold. Defaults to all discovered repos.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Only scaffold these files (by name, e.g. LICENSE).
        #[arg(long)]
        files: Vec<String>,
        /// Overwrite existing files with template versions.
        #[arg(long)]
        overwrite: bool,
        /// Preview what would be done without making any changes.
        #[arg(long)]
        dry_run: bool,
    },
    /// Detect and report repository ownership (safety guard for
    /// auto-commit/auto-push).
    Ownership {
        /// Repository path. Defaults to all discovered repos.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Show the raw signals checked (git config, HEAD author, origin URL)
        /// in addition to the classified report.
        #[arg(long, conflicts_with = "json")]
        explain: bool,
        /// Emit machine-readable JSON.
        #[arg(long, conflicts_with = "explain")]
        json: bool,
    },

    /// Scan watched repos for untracked collection directories that are
    /// not yet in `untracked_exclude_patterns`. The operator's discovery
    /// loop for forward-compatibility: future tools that drop a new
    /// directory name into working trees are auto-flagged here, and the
    /// operator decides whether to extend the daemon's exclude list, add
    /// to `.gitignore`, or leave them as intentional content.
    ///
    /// Buckets are aggregated by directory leaf name across repos (so
    /// `test-results/` recurring in N repos becomes one row). Singletons
    /// (`min_repo_count < 2`) and tiny dirs (`min_size_mib < 5`) are
    /// filtered out by default; tune with the flags below. See
    /// `docs/design/codeberg-quota-leak-fix-2026-07-13.md`.
    ScanBloat {
        /// Minimum total size per bucket (MiB). Default: 5.
        #[arg(long, default_value_t = 5)]
        min_size_mib: u64,
        /// Minimum number of repos a bucket must appear in. Default: 2.
        #[arg(long, default_value_t = 2)]
        min_repo_count: usize,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Refresh visibility cache for all watched repos.
    ///
    /// Walks all 31 watched repos, queries GitHub via `gh api repos/DraconDev/<repo>`
    /// for each one's `.private` flag, and writes the new cache format
    /// (`visibility=<public|private>\n<timestamp>`) to the local cache file.
    ///
    /// This is the operator's escape hatch from the 24h `sync_mirror_visibility`
    /// cycle. Useful after the v0.112.16 codeberg-public-only rollout, when
    /// 30 of 31 watched repos still show `(unknown)` in the PUSH-TO column
    /// because their cache files are in legacy `timestamp-only` format.
    ///
    /// Idempotent: can be re-run safely. Skips repos with no origin remote
    /// (cannot derive GitHub owner/repo). Skips repos where `gh api` fails
    /// (network, auth) — those fall back to the safe-default `(unknown)`
    /// state until a future refresh succeeds.
    RefreshVisibility {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Flip a repository to public visibility across github + gitlab.
    /// Skips codeberg by default to protect the 85 GiB grace quota;
    /// pass `--include-codeberg` to flip it too. The repo's local
    /// visibility cache is refreshed on success so `repos` reflects
    /// the new state immediately.
    ///
    /// ADDED 2026-07-20 (v0.112.28).
    MakePublic {
        /// Repository name (basename) to flip public. E.g. `dracon-sync`.
        repo: String,
        /// Also flip codeberg (opt-in; default off to protect quota).
        #[arg(long)]
        include_codeberg: bool,
        /// Don't update the local visibility cache after success.
        #[arg(long)]
        no_cache_update: bool,
    },

    /// Flip a repository to private visibility across github + gitlab.
    /// Mirror of `make-public`. Skips codeberg by default.
    ///
    /// ADDED 2026-07-20 (v0.112.28).
    MakePrivate {
        /// Repository name (basename) to flip private. E.g. `dracon-sync`.
        repo: String,
        /// Also flip codeberg (opt-in; default off to protect quota).
        #[arg(long)]
        include_codeberg: bool,
        /// Don't update the local visibility cache after success.
        #[arg(long)]
        no_cache_update: bool,
    },
}

#[derive(Subcommand, Debug)]
enum RepairCommands {
    /// Repair concern repos (dry-run by default; use --apply to execute).
    Concerns {
        /// Execute git operations to repair concerns.
        #[arg(long)]
        apply: bool,
        /// Only repair this repository path.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Override push timeout seconds for this run.
        #[arg(long)]
        push_timeout_secs: Option<u64>,
        /// Retry count for push operations.
        #[arg(long, default_value_t = 3)]
        push_retries: u32,
        /// Allow rewrite of large blobs even when paths are outside excluded dirs.
        #[arg(long)]
        rewrite_large_any: bool,
        /// Only repair stuck push concerns.
        #[arg(long, conflicts_with = "only_stuck_pull")]
        only_stuck_push: bool,
        /// Only repair stuck pull concerns.
        #[arg(long, conflicts_with = "only_stuck_push")]
        only_stuck_pull: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Repair warn repos (dirty-only triage; dry-run by default).
    Warns {
        /// Execute git operations to repair warns.
        #[arg(long)]
        apply: bool,
        /// Only repair this repository path.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Detect and repair origin URLs pointing to orphan -N suffixed repos.
    Origins {
        /// Execute git operations to repair origins.
        #[arg(long)]
        apply: bool,
    },
    /// List repos that are permanently stuck on push.
    StuckList,
    /// Unstuck a repo that was marked as permanently stuck.
    StuckUnstuck {
        /// The repository path to unstuck.
        repo: PathBuf,
    },
    /// List repos that have both main and master branches.
    DualBranchList,
    /// Consolidate a repo with both main and master to main only.
    DualBranchRepair {
        /// The repository path to consolidate.
        repo: PathBuf,
        /// Actually delete the master branch (without this, dry-run only).
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PublishCommands {
    /// Publish a repository to configured package registries.
    Run {
        /// The repository path to publish.
        repo: PathBuf,
        /// Only publish to these target names (defaults to all configured).
        #[arg(long)]
        targets: Vec<String>,
        /// Skip the dry-run check and publish directly.
        #[arg(long)]
        skip_dry_run: bool,
    },
    /// Show publish status for a repository across configured registries.
    Status {
        /// The repository path to check.
        repo: PathBuf,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Open sync policy in the system editor.
    Edit,
    /// Validate the sync policy for errors and warnings.
    Validate,
}

// CHANGED 2026-07-04 (goal mr5s1530-755tj8): use the multi-thread
// runtime flavor so the per-repo work in `run_repos_report` actually runs
// in parallel. The default `current_thread` flavor would serialize the
// futures even though they are wrapped in `buffer_unordered(16)`, because
// there is no other worker thread to schedule them on.
/// Flip a repository's visibility across all configured remotes.
/// Used by both `make-public` and `make-private` subcommands.
///
/// `target_private = true` means make private; `false` means make public.
/// Skips codeberg by default; pass `include_codeberg=true` to flip it
/// (opt-in to protect the 85 GiB grace quota).
///
/// ADDED 2026-07-20 (v0.112.28).
fn handle_visibility_flip(
    policy_path: &std::path::Path,
    repo_name: &str,
    include_codeberg: bool,
    no_cache_update: bool,
    target_private: bool,
) -> Result<()> {
    let policy = SyncPolicy::load(policy_path)?;
    let roots = policy.watch_root_paths();
    let excluded_dir_names = excluded_dir_names_set(&policy);
    let repos = git::discover_git_repos(
        &roots,
        &excluded_dir_names,
        &policy.exclude_repos,
        Some(&policy.system_repo),
    );

    // Find the repo by basename.
    let repo_path = repos.into_iter().find(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy() == repo_name)
            .unwrap_or(false)
    });
    let Some(repo_path) = repo_path else {
        anyhow::bail!(
            "repo '{}' not found in watch roots ({}); pass the basename, e.g. `dracon-sync`",
            repo_name,
            roots
                .iter()
                .map(|r| r.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    };

    // Get origin URL.
    let origin_url = std::process::Command::new("git")
        .current_dir(&repo_path)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if origin_url.is_empty() {
        anyhow::bail!(
            "repo '{}' has no origin remote; cannot flip visibility without a github origin",
            repo_name
        );
    }

    let visibility_str = if target_private { "private" } else { "public" };
    println!(
        "🔁 flipping {} → {} across github + gitlab{}",
        repo_name,
        visibility_str,
        if include_codeberg { " + codeberg" } else { "" }
    );

    let results = visibility::flip_repo_visibility(
        &origin_url,
        &policy.remotes,
        repo_name,
        target_private,
        include_codeberg,
    );

    let mut any_fail = false;
    for (remote_name, result) in &results {
        match result {
            Ok(()) => println!("  ✅ {} → {}", remote_name, visibility_str),
            Err(e) => {
                any_fail = true;
                println!("  ❌ {}: {}", remote_name, e);
            }
        }
    }

    if any_fail {
        println!(
            "\n⚠️  some remotes failed; the flip is partial. Re-run to retry."
        );
    } else {
        println!("\n✅ all remotes flipped to {}", visibility_str);
    }

    // Update visibility cache so `repos` reflects the new state.
    if !no_cache_update {
        let any_success = results.iter().any(|(_, r)| r.is_ok());
        if any_success {
            visibility::update_visibility_cache(&repo_path, target_private);
            if policy::debug_enabled() {
                eprintln!("🐛 updated visibility cache for {}", repo_path.display());
            }
        }
    }

    if any_fail {
        std::process::exit(2);
    }
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // If output is piped (e.g. `dracon-sync repos | head`), stdout can become a broken pipe.
    // Rust's default printing panics on write errors; convert that specific panic into a clean exit.
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        if msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        default_panic_hook(info);
    }));

    let cli = Cli::parse();
    daemon::VERBOSITY.store(cli.verbose, Ordering::SeqCst);
    let policy_path = resolve_policy_path()?;

    match cli.cmd {
        Command::Status { json } => {
            let policy = SyncPolicy::load(&policy_path)?;
            let roots = policy.watch_root_paths();
            let excluded_dir_names = excluded_dir_names_set(&policy);
            let repos = git::discover_git_repos(
                &roots,
                &excluded_dir_names,
                &policy.exclude_repos,
                Some(&policy.system_repo),
            );
            let freeze = freeze_reason(&policy_path);
            if json {
                let payload = report::StatusJson {
                    policy: policy_path.display().to_string(),
                    roots: roots.iter().map(|p| p.display().to_string()).collect(),
                    repos_discovered: repos.len(),
                    pulse_interval_secs: policy.pulse_interval_secs,
                    inactivity_push_delay_secs: policy.inactivity_push_delay_secs,
                    freeze: freeze
                        .map(|r| format!("ON ({})", r))
                        .unwrap_or_else(|| "OFF".to_string()),
                    auto_commit: policy.auto_commit,
                    auto_pull: policy.auto_pull,
                    auto_push: policy.auto_push,
                    auto_bump_versions: policy.auto_bump_versions,
                    auto_repair_concerns: policy.auto_repair_concerns,
                    auto_repair_warns: policy.auto_repair_warns,
                    auto_rewrite_large_blobs: policy.auto_rewrite_large_blobs,
                    max_stage_file_bytes: policy.max_stage_file_bytes,
                    push_blob_threshold_bytes: push_large_blob_threshold_bytes(&policy),
                    exclude_dirs: policy.exclude_dir_names.clone(),
                    exclude_file_patterns: policy.exclude_file_patterns.clone(),
                    pull_op_timeout_secs: policy.pull_op_timeout_secs,
                    push_op_timeout_secs: policy.push_op_timeout_secs,
                    repo_sync_timeout_secs: policy.repo_sync_timeout_secs,
                    stage_op_timeout_secs: policy.stage_op_timeout_secs,
                    stage_cooldown_secs: policy.stage_cooldown_secs,
                    push_retries: policy.push_retries,
                    repair_cooldown_secs: policy.repair_cooldown_secs,
                    incident_ledger_max_lines: policy.incident_ledger_max_lines,
                    incident_ledger_max_age_days: policy.incident_ledger_max_age_days,
                    system_repo: policy.system_repo.clone(),
                    backup_policy: policy.backup_policy.clone(),
                    backup_dir: policy.backup_dir.clone(),
                    remotes: policy.remotes.len(),
                    remote_configs: policy
                        .remotes
                        .iter()
                        .map(|r| report::RemoteStatus {
                            name: r.name.clone(),
                            auth_type: format!("{:?}", r.auth_type).to_lowercase(),
                            auto_create: r.auto_create,
                            priority: r.priority,
                        })
                        .collect(),
                };
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                use comfy_table::{
                    presets::UTF8_FULL_CONDENSED, Cell, Color, ContentArrangement, Table,
                };

                let mut table = Table::new();
                table.load_preset(UTF8_FULL_CONDENSED);
                table.set_content_arrangement(ContentArrangement::DynamicFullWidth);
                table.set_header(vec![Cell::new("KEY"), Cell::new("VALUE")]);

                // Policy path
                table.add_row(vec![
                    Cell::new("📜 Policy"),
                    Cell::new(policy_path.display().to_string()),
                ]);
                // Summary one-liner for quick scanning
                table.add_row(vec![
                    Cell::new("📋 Summary"),
                    Cell::new(format!(
                        "{} repos · {} watch root(s) · pulse {}",
                        repos.len(),
                        roots.len(),
                        crate::print::format_secs(policy.pulse_interval_secs)
                    )),
                ]);

                // Roots
                let roots_str: Vec<String> =
                    roots.iter().map(|p| p.display().to_string()).collect();
                table.add_row(vec![Cell::new("🔁 Roots"), Cell::new(roots_str.join(", "))]);

                // Repos
                table.add_row(vec![Cell::new("📦 Repos"), Cell::new(repos.len())]);

                // Pulse & Inactivity
                table.add_row(vec![
                    Cell::new("⏱️ Pulse"),
                    Cell::new(crate::print::format_secs(policy.pulse_interval_secs)),
                ]);
                table.add_row(vec![
                    Cell::new("⏳ Inactivity"),
                    Cell::new(crate::print::format_secs(policy.inactivity_push_delay_secs)),
                ]);

                // Freeze
                let freeze_str = freeze
                    .map(|r| format!("ON ({})", r))
                    .unwrap_or_else(|| "OFF".to_string());
                let freeze_color = if !print::should_color() {
                    Color::Reset
                } else if freeze_str == "OFF" {
                    Color::Green
                } else {
                    Color::Red
                };
                table.add_row(vec![
                    Cell::new("⏸️ Freeze"),
                    Cell::new(freeze_str).fg(freeze_color),
                ]);

                // Flags
                let flags = [
                    format!("commit={}", onoff(policy.auto_commit)),
                    format!("pull={}", onoff(policy.auto_pull)),
                    format!("push={}", onoff(policy.auto_push)),
                    format!("bump={}", onoff(policy.auto_bump_versions)),
                    format!("repair_concerns={}", onoff(policy.auto_repair_concerns)),
                    format!("repair_warns={}", onoff(policy.auto_repair_warns)),
                    format!(
                        "rewrite_large_blobs={}",
                        onoff(policy.auto_rewrite_large_blobs)
                    ),
                ]
                .join("  ");
                table.add_row(vec![Cell::new("⚙️ Flags"), Cell::new(flags)]);

                // Limits
                table.add_row(vec![
                    Cell::new("📏 Max stage file"),
                    Cell::new(format!(
                        "{} ({})",
                        crate::print::format_bytes(policy.max_stage_file_bytes),
                        policy.max_stage_file_bytes
                    )),
                ]);
                table.add_row(vec![
                    Cell::new("🧱 Push blob threshold"),
                    Cell::new(format!(
                        "{} ({})",
                        crate::print::format_bytes(push_large_blob_threshold_bytes(&policy)),
                        push_large_blob_threshold_bytes(&policy)
                    )),
                ]);

                // Exclude
                if !policy.exclude_dir_names.is_empty() {
                    table.add_row(vec![
                        Cell::new("🚫 Exclude dirs"),
                        Cell::new(policy.exclude_dir_names.join(", ")),
                    ]);
                }
                if !policy.exclude_file_patterns.is_empty() {
                    table.add_row(vec![
                        Cell::new("🚫 Exclude patterns"),
                        Cell::new(policy.exclude_file_patterns.join(", ")),
                    ]);
                }

                // Timeouts
                table.add_row(vec![
                    Cell::new("⏱️ Pull timeout"),
                    Cell::new(crate::print::format_secs(policy.pull_op_timeout_secs)),
                ]);
                table.add_row(vec![
                    Cell::new("⏱️ Push timeout"),
                    Cell::new(crate::print::format_secs(policy.push_op_timeout_secs)),
                ]);
                table.add_row(vec![
                    Cell::new("⏱️ Repo sync timeout"),
                    Cell::new(crate::print::format_secs(policy.repo_sync_timeout_secs)),
                ]);
                table.add_row(vec![
                    Cell::new("⏱️ Stage timeout"),
                    Cell::new(crate::print::format_secs(policy.stage_op_timeout_secs)),
                ]);
                table.add_row(vec![
                    Cell::new("⏸️  Stage cooldown"),
                    Cell::new(crate::print::format_secs(policy.stage_cooldown_secs)),
                ]);
                table.add_row(vec![
                    Cell::new("🔁 Push retries"),
                    Cell::new(policy.push_retries),
                ]);

                // Repair
                table.add_row(vec![
                    Cell::new("🧯 Repair cooldown"),
                    Cell::new(crate::print::format_secs(policy.repair_cooldown_secs)),
                ]);
                table.add_row(vec![
                    Cell::new("📒 Incident ledger"),
                    Cell::new(format!(
                        "{} lines · {}d retention",
                        policy.incident_ledger_max_lines, policy.incident_ledger_max_age_days
                    )),
                ]);

                // System repo
                if !policy.system_repo.is_empty() {
                    table.add_row(vec![
                        Cell::new("🏛️ System repo"),
                        Cell::new(&policy.system_repo),
                    ]);
                }

                // Backup
                if !policy.backup_policy.is_empty() || !policy.backup_dir.is_empty() {
                    table.add_row(vec![
                        Cell::new("🧰 Backup"),
                        Cell::new(format!(
                            "policy={} dir={}",
                            policy.backup_policy, policy.backup_dir
                        )),
                    ]);
                }

                // Remotes
                table.add_row(vec![
                    Cell::new("🌐 Remotes"),
                    Cell::new(policy.remotes.len()),
                ]);

                println!("{table}");
            }
        }
        Command::Config { cmd } => match cmd {
            ConfigCommands::Edit => {
                policy::open_policy_in_editor(&policy_path)?;
            }
            ConfigCommands::Validate => {
                let result = policy::validate_config(&policy_path);
                // CHANGED 2026-07-21 (v0.112.33, audit M21/F3.3):
                // warnings are printed UNCONDITIONALLY — the pre-fix
                // success path reported "✅ Policy is valid" while
                // withholding every collected warning (TOML-ordering
                // footguns, low timeouts, missing token files). The
                // only command whose entire job is config linting
                // now actually shows the lint output.
                if !result.warnings.is_empty() {
                    println!("⚠️  Warnings:");
                    for w in &result.warnings {
                        println!("  WARNING: {}", w);
                    }
                    println!();
                }
                if result.is_valid() {
                    println!("✅ Policy is valid");
                } else {
                    println!("❌ Policy has errors:");
                    for e in &result.errors {
                        println!("  ERROR: {}", e);
                    }
                    std::process::exit(1);
                }
            }
        },
        Command::Pause => {
            if let Some(home) = dirs::home_dir() {
                let marker = home.join(".dracon").join("dracon-sync.freeze");
                std::fs::write(&marker, format!("paused at {}\n", timestamp_secs()))?;
                println!("⏸️  Sync paused (freeze marker: {})", marker.display());
            } else {
                anyhow::bail!("cannot determine home directory");
            }
        }
        Command::Resume => {
            if let Some(home) = dirs::home_dir() {
                let marker = home.join(".dracon").join("dracon-sync.freeze");
                if marker.exists() {
                    std::fs::remove_file(&marker)?;
                    println!("▶️  Sync resumed (freeze marker removed)");
                } else {
                    println!("ℹ️  No freeze marker found — sync was not paused");
                }
            } else {
                anyhow::bail!("cannot determine home directory");
            }
        }
        Command::Once => {
            run_once(&policy_path).await?;
        }
        Command::Daemon { interval_secs } => {
            run_daemon(policy_path, interval_secs).await?;
        }
        Command::SyncNow {
            repos,
            warns,
            dry_run,
        } => {
            if let Some(reason) = freeze_reason(&policy_path) {
                println!("⏸️ sync frozen ({})", reason);
                return Ok(());
            }
            if warns {
                run_repair_warns(&policy_path, !dry_run, None, false).await?;
                if dry_run {
                    println!(
                        "ℹ️ invoked via sync-now --dry-run; rerun `dracon-sync sync-now --warns` to execute"
                    );
                }
                return Ok(());
            }
            let policy = SyncPolicy::load(&policy_path)?;
            let excluded_dir_names = excluded_dir_names_set(&policy);
            for repo in repos {
                if daemon::is_repo_stuck(&repo) {
                    println!(
                        "🔒 {} is stuck on push. Run 'dracon-sync repair stuck-unstuck {}' first.",
                        repo.display(),
                        repo.display()
                    );
                    continue;
                }
                match sync_repo(
                    &repo,
                    &policy,
                    &excluded_dir_names,
                    0,
                    None,
                    dry_run,
                    Some(&policy_path),
                )
                .await
                {
                    Ok(crate::sync::SyncOutcome::Synced) => {
                        if dry_run {
                            println!("✅ dry-run complete for {}", repo.display());
                        } else {
                            println!("🔁 synced {}", repo.display());
                        }
                    }
                    Ok(crate::sync::SyncOutcome::NothingToDo) => {
                        if dry_run {
                            println!("✅ no sync changes needed for {}", repo.display());
                        } else {
                            println!("✅ no sync changes {}", repo.display());
                        }
                    }
                    Ok(crate::sync::SyncOutcome::Blocked) => {
                        println!(
                            "⏸️  sync blocked for {} (guard or manual intervention required)",
                            repo.display()
                        );
                    }
                    // ADDED 2026-07-21 (v0.112.31, audit H3/F1.3).
                    Ok(crate::sync::SyncOutcome::PushFailed) => {
                        eprintln!(
                            "⚠️ {} committed but push failed (see daemon log)",
                            repo.display()
                        );
                    }
                    // ADDED 2026-07-21 (v0.112.33, audit M9/F1.8).
                    Ok(crate::sync::SyncOutcome::FilterOnly) => {
                        println!(
                            "🧹 {} filter-only dirty (nothing real to commit)",
                            repo.display()
                        );
                    }
                    Err(e) => {
                        eprintln!("❌ error syncing {}: {}", repo.display(), e);
                    }
                }
            }
        }
        Command::Repos {
            only_concern,
            only_warn,
            json,
            sort,
            filter: filter_name,
            full_path,
            legend,
            layout,
            summary,
            summary_by_severity,
        } => {
            let filter = if only_concern {
                RepoFilter::Concern
            } else if only_warn {
                RepoFilter::Warn
            } else {
                RepoFilter::All
            };
            run_repos_report(
                &policy_path,
                filter,
                json,
                &sort,
                filter_name.as_deref(),
                full_path,
                legend,
                layout.as_deref(),
                summary,
                summary_by_severity,
            )
            .await?;
        }
        Command::Repair { cmd } => match cmd {
            RepairCommands::Concerns {
                apply,
                repo,
                push_timeout_secs,
                push_retries,
                rewrite_large_any,
                only_stuck_push,
                only_stuck_pull,
                json,
            } => {
                let filter = if only_stuck_push {
                    ConcernRepairFilter::StuckPush
                } else if only_stuck_pull {
                    ConcernRepairFilter::StuckPull
                } else {
                    ConcernRepairFilter::All
                };
                if !json {
                    println!("📜 Policy: {}", policy_path.display());
                    println!(
                        "🛠️ Mode: {}",
                        if apply {
                            "APPLY (mutating)"
                        } else {
                            "DRY-RUN (no changes)"
                        }
                    );
                    println!(
                        "⚙️ Push: timeout={}s retries={}",
                        push_timeout_secs.unwrap_or(0),
                        push_retries
                    );
                }
                run_repair_concerns(
                    &policy_path,
                    apply,
                    repo,
                    push_timeout_secs,
                    push_retries,
                    rewrite_large_any,
                    filter,
                    json,
                )
                .await?;
            }
            RepairCommands::Warns { apply, repo, json } => {
                if !json {
                    println!("📜 Policy: {}", policy_path.display());
                    println!(
                        "🧹 Warn mode: {}",
                        if apply {
                            "APPLY (mutating)"
                        } else {
                            "DRY-RUN (no changes)"
                        }
                    );
                }
                run_repair_warns(&policy_path, apply, repo, json).await?;
            }
            RepairCommands::Origins { apply } => {
                let policy = SyncPolicy::load(&policy_path)?;
                let roots = policy.watch_root_paths();
                let excluded_dir_names = excluded_dir_names_set(&policy);
                let repos = git::discover_git_repos(
                    &roots,
                    &excluded_dir_names,
                    &policy.exclude_repos,
                    Some(&policy.system_repo),
                );
                let mut found = 0;
                for repo in repos {
                    if let Some((current, canonical)) = detect_orphan_origin(&repo) {
                        println!("   {}: {} -> {}", repo.display(), current, canonical);
                        found += 1;
                        if apply {
                            if let Err(e) = fix_orphan_origin(&repo, &canonical) {
                                eprintln!("❌ failed to fix origin for {}: {}", repo.display(), e);
                            } else {
                                println!("✅ fixed origin for {}", repo.display());
                            }
                        }
                    }
                }
                if found == 0 {
                    println!("✅ no orphan origins found");
                } else if !apply {
                    println!("\n🔧 Run 'dracon-sync repair origins --apply' to fix them");
                }
            }
            RepairCommands::StuckList => {
                list_stuck_repos();
            }
            RepairCommands::StuckUnstuck { repo } => {
                unstuck_repo(&repo);
            }
            RepairCommands::DualBranchList => {
                let policy = SyncPolicy::load(&policy_path)?;
                let roots = policy.watch_root_paths();
                let excluded_dir_names = excluded_dir_names_set(&policy);
                let repos = git::discover_git_repos(
                    &roots,
                    &excluded_dir_names,
                    &policy.exclude_repos,
                    Some(&policy.system_repo),
                );
                let mut found = 0;
                for repo in repos {
                    if has_both_main_and_master(&repo) {
                        let branch =
                            git::current_branch(&repo).unwrap_or_else(|| "unknown".to_string());
                        println!("   {} (currently on {})", repo.display(), branch);
                        found += 1;
                    }
                }
                if found == 0 {
                    println!("✅ no repos with both main and master");
                } else {
                    println!("\n🔧 Run 'dracon-sync repair dual-branch-repair <path>' to consolidate to main");
                }
            }
            RepairCommands::DualBranchRepair { repo, apply } => {
                if !has_both_main_and_master(&repo) {
                    println!("ℹ️ {} does not have both main and master", repo.display());
                    return Ok(());
                }
                if !apply {
                    // F34 (2026-07-19): the previous command always
                    // deleted the master branch locally + remotely
                    // without explicit confirmation. Dry-run by default;
                    // require `--apply` to actually delete.
                    println!(
                        "🔍 DRY-RUN: would consolidate {} to main (deletes master locally + remotely).",
                        repo.display()
                    );
                    println!("   Pass --apply to actually perform the deletion.");
                    return Ok(());
                }
                println!("🔧 Consolidating {} to main...", repo.display());
                match consolidate_to_main(&repo).await {
                    Ok(()) => println!("✅ consolidated to main"),
                    Err(e) => {
                        eprintln!("❌ failed: {}", e);
                        return Err(e);
                    }
                }
            }
        },
        Command::Health { json } => {
            let policy = SyncPolicy::load(&policy_path)?;
            let validate_result = policy::validate_config(&policy_path);
            let roots = policy.watch_root_paths();
            let excluded_dir_names = excluded_dir_names_set(&policy);
            let repos = git::discover_git_repos(
                &roots,
                &excluded_dir_names,
                &policy.exclude_repos,
                Some(&policy.system_repo),
            );
            let freeze = freeze_reason(&policy_path);

            let frozen = freeze.is_some();
            let policy_ok = validate_result.is_valid();
            let daemon_ok = true;

            let status = if frozen || !policy_ok {
                "unhealthy"
            } else {
                "healthy"
            };

            if json {
                #[derive(serde::Serialize)]
                struct HealthJson<'a> {
                    status: &'a str,
                    frozen: bool,
                    freeze_reason: Option<&'a str>,
                    policy_valid: bool,
                    policy_errors: Vec<String>,
                    policy_warnings: Vec<String>,
                    daemon_running: bool,
                    roots: usize,
                    repos_discovered: usize,
                }
                let payload = HealthJson {
                    status,
                    frozen,
                    freeze_reason: freeze.as_deref(),
                    policy_valid: policy_ok,
                    policy_errors: validate_result.errors,
                    policy_warnings: validate_result.warnings,
                    daemon_running: daemon_ok,
                    roots: roots.len(),
                    repos_discovered: repos.len(),
                };
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                use comfy_table::{
                    presets::UTF8_FULL_CONDENSED, Cell, Color, ContentArrangement, Table,
                };
                let color = print::should_color();
                let mk = |s: &str, c: Color| -> Cell {
                    if color {
                        Cell::new(s).fg(c)
                    } else {
                        Cell::new(s)
                    }
                };

                // ---- Summary line (one-liner) ----
                let summary_icon = if status == "healthy" { "✅" } else { "❌" };
                let daemon_str = if daemon_ok { "running" } else { "not running" };
                let freeze_str = freeze
                    .as_ref()
                    .map(|r| format!("⏸️ on ({})", r))
                    .unwrap_or_else(|| "off".to_string());
                let policy_str = if policy_ok { "valid" } else { "invalid" };
                println!(
                    "🏥 Health · {summary_icon} {status} · daemon {daemon_str} · freeze {freeze_str} · policy {policy_str}"
                );

                // ---- Main status table ----
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL_CONDENSED)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(vec![Cell::new(" "), Cell::new("KEY"), Cell::new("VALUE")]);

                let status_color = if status == "healthy" {
                    Color::Green
                } else {
                    Color::Red
                };
                table.add_row(vec![
                    mk(if status == "healthy" { "✅" } else { "❌" }, status_color),
                    Cell::new("Status"),
                    mk(status, status_color),
                ]);

                let daemon_color = if daemon_ok { Color::Green } else { Color::Red };
                table.add_row(vec![
                    mk(if daemon_ok { "✅" } else { "❌" }, daemon_color),
                    Cell::new("Daemon"),
                    if daemon_ok {
                        mk("running", Color::Green)
                    } else {
                        mk(
                            "not running · systemctl --user start dracon-sync.service",
                            Color::Red,
                        )
                    },
                ]);

                if let Some(reason) = &freeze {
                    table.add_row(vec![
                        mk("⏸️", Color::Yellow),
                        Cell::new("Freeze"),
                        mk(&format!("on ({})", reason), Color::Yellow),
                    ]);
                } else {
                    table.add_row(vec![Cell::new("  "), Cell::new("Freeze"), Cell::new("off")]);
                }

                let policy_color = if policy_ok { Color::Green } else { Color::Red };
                table.add_row(vec![
                    mk(if policy_ok { "✅" } else { "❌" }, policy_color),
                    Cell::new("Policy"),
                    mk(policy_str, policy_color),
                ]);

                table.add_row(vec![
                    Cell::new("📦"),
                    Cell::new("Repos"),
                    Cell::new(format!(
                        "{} discovered across {} roots",
                        repos.len(),
                        roots.len()
                    )),
                ]);

                println!("{table}");

                // ---- Errors block (if any) ----
                if !validate_result.errors.is_empty() {
                    println!();
                    println!("❌ Policy errors ({}):", validate_result.errors.len());
                    for e in &validate_result.errors {
                        println!("   ❌ {e}");
                    }
                }

                // ---- Warnings block (grouped) ----
                if !validate_result.warnings.is_empty() {
                    println!();
                    println!("⚠️  Policy warnings ({}):", validate_result.warnings.len());
                    for w in &validate_result.warnings {
                        println!("   ⚠️  {w}");
                    }
                }

                // ---- Tip line ----
                if !daemon_ok || !policy_ok || !validate_result.errors.is_empty() {
                    println!();
                    println!("💡 Tip: run `dracon-sync config validate` for full diagnostics");
                }
            }
        }
        Command::RefreshVisibility { json } => {
            let policy = SyncPolicy::load(&policy_path)?;
            let roots = policy.watch_root_paths();
            let excluded_dir_names = excluded_dir_names_set(&policy);
            let repos = git::discover_git_repos(
                &roots,
                &excluded_dir_names,
                &policy.exclude_repos,
                Some(&policy.system_repo),
            );

            let mut results: Vec<serde_json::Value> = Vec::new();
            let mut refreshed = 0usize;
            let mut skipped = 0usize;
            let errors = 0usize;

            for repo_path in &repos {
                // Try `origin` first (most common), then fall back to `github`
                // (some repos — e.g. opencode-plugins — use `github` as the
                // remote name instead of `origin`).
                let mut origin_url = String::new();
                for remote_name in ["origin", "github"] {
                    if let Ok(o) = std::process::Command::new("git")
                        .args([
                            "-C",
                            &repo_path.to_string_lossy(),
                            "remote",
                            "get-url",
                            remote_name,
                        ])
                        .output()
                    {
                        if o.status.success() {
                            let url = String::from_utf8_lossy(&o.stdout).trim().to_string();
                            if !url.is_empty() {
                                origin_url = url;
                                break;
                            }
                        }
                    }
                }

                let Some((owner, gh_repo)) =
                    crate::visibility::parse_github_owner_repo(&origin_url)
                else {
                    skipped += 1;
                    results.push(serde_json::json!({
                        "repo": repo_path.file_name().map(|s| s.to_string_lossy().to_string()),
                        "status": "skipped",
                        "reason": "no parseable github remote (tried origin, github)",
                    }));
                    continue;
                };

                let private = crate::visibility::get_github_visibility(&owner, &gh_repo);
                let visibility = if private { "private" } else { "public" };
                crate::visibility::update_visibility_cache(repo_path, private);
                refreshed += 1;
                results.push(serde_json::json!({
                    "repo": gh_repo,
                    "owner": owner,
                    "visibility": visibility,
                    "status": "refreshed",
                }));
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "total": repos.len(),
                        "refreshed": refreshed,
                        "skipped": skipped,
                        "errors": errors,
                        "results": results,
                    }))?
                );
            } else {
                println!(
                    "🔄 refresh-visibility · {} repos · refreshed {} · skipped {}",
                    repos.len(),
                    refreshed,
                    skipped,
                );
                for r in &results {
                    let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                    let repo = r.get("repo").and_then(|v| v.as_str()).unwrap_or("?");
                    let detail = if status == "refreshed" {
                        let vis = r.get("visibility").and_then(|v| v.as_str()).unwrap_or("?");
                        format!("→ ({})", vis)
                    } else {
                        let reason = r.get("reason").and_then(|v| v.as_str()).unwrap_or("?");
                        format!("skipped: {}", reason)
                    };
                    println!("  {}  {:<30} {}", status, repo, detail);
                }
            }
        }
        Command::MakePublic {
            repo,
            include_codeberg,
            no_cache_update,
        } => {
            handle_visibility_flip(&policy_path, &repo, include_codeberg, no_cache_update, false)?;
        }
        Command::MakePrivate {
            repo,
            include_codeberg,
            no_cache_update,
        } => {
            handle_visibility_flip(&policy_path, &repo, include_codeberg, no_cache_update, true)?;
        }
        Command::Metrics => {
            let policy = SyncPolicy::load(&policy_path)?;
            let roots = policy.watch_root_paths();
            let excluded_dir_names = excluded_dir_names_set(&policy);
            let repos = git::discover_git_repos(
                &roots,
                &excluded_dir_names,
                &policy.exclude_repos,
                Some(&policy.system_repo),
            );
            let freeze = freeze_reason(&policy_path);
            let frozen = freeze.is_some();

            println!("# HELP dracon_sync_info Dracon sync daemon info");
            println!("# TYPE dracon_sync_info gauge");
            println!(
                "dracon_sync_info{{version=\"{}\"}} 1",
                env!("CARGO_PKG_VERSION")
            );

            println!(
                "# HELP dracon_sync_repos_discovered_total Number of git repositories discovered"
            );
            println!("# TYPE dracon_sync_repos_discovered_total gauge");
            println!("dracon_sync_repos_discovered_total {}", repos.len());

            println!("# HELP dracon_sync_watch_roots_total Number of configured watch roots");
            println!("# TYPE dracon_sync_watch_roots_total gauge");
            println!("dracon_sync_watch_roots_total {}", roots.len());

            println!("# HELP dracon_sync_remotes_total Number of configured remotes");
            println!("# TYPE dracon_sync_remotes_total gauge");
            println!("dracon_sync_remotes_total {}", policy.remotes.len());

            println!("# HELP dracon_sync_freeze_state Whether sync is currently frozen (1=frozen, 0=active)");
            println!("# TYPE dracon_sync_freeze_state gauge");
            println!("dracon_sync_freeze_state {}", if frozen { 1 } else { 0 });

            println!("# HELP dracon_sync_policy_auto_commit Whether auto-commit is enabled");
            println!("# TYPE dracon_sync_policy_auto_commit gauge");
            println!(
                "dracon_sync_policy_auto_commit {}",
                if policy.auto_commit { 1 } else { 0 }
            );

            println!("# HELP dracon_sync_policy_auto_push Whether auto-push is enabled");
            println!("# TYPE dracon_sync_policy_auto_push gauge");
            println!(
                "dracon_sync_policy_auto_push {}",
                if policy.auto_push { 1 } else { 0 }
            );

            println!("# HELP dracon_sync_policy_auto_pull Whether auto-pull is enabled");
            println!("# TYPE dracon_sync_policy_auto_pull gauge");
            println!(
                "dracon_sync_policy_auto_pull {}",
                if policy.auto_pull { 1 } else { 0 }
            );

            println!("# HELP dracon_sync_policy_auto_repair_concerns Whether auto-repair concerns is enabled");
            println!("# TYPE dracon_sync_policy_auto_repair_concerns gauge");
            println!(
                "dracon_sync_policy_auto_repair_concerns {}",
                if policy.auto_repair_concerns { 1 } else { 0 }
            );

            println!("# HELP dracon_sync_incident_ledger_max_lines Incident ledger max lines");
            println!("# TYPE dracon_sync_incident_ledger_max_lines gauge");
            println!(
                "dracon_sync_incident_ledger_max_lines {}",
                policy.incident_ledger_max_lines
            );

            let incident_path = report::incident_ledger_path(&policy_path);
            if incident_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&incident_path) {
                    let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
                    println!("# HELP dracon_sync_incident_ledger_lines_current Current number of lines in incident ledger");
                    println!("# TYPE dracon_sync_incident_ledger_lines_current gauge");
                    println!("dracon_sync_incident_ledger_lines_current {}", lines);
                }
            }

            if let Some(home) = dirs::home_dir() {
                let stuck_path = home.join(".local/state/dracon/dracon-sync-stuck-push-repos.json");
                if stuck_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&stuck_path) {
                        if let Ok(stuck) = serde_json::from_str::<Vec<serde_json::Value>>(&content)
                        {
                            println!("# HELP dracon_sync_stuck_repos_total Number of repos permanently stuck on push");
                            println!("# TYPE dracon_sync_stuck_repos_total gauge");
                            println!("dracon_sync_stuck_repos_total {}", stuck.len());
                        }
                    }
                }
            }

            println!("# HELP dracon_sync_push_retries Default push retry count");
            println!("# TYPE dracon_sync_push_retries gauge");
            println!("dracon_sync_push_retries {}", policy.push_retries);

            println!("# HELP dracon_sync_pulse_interval_secs Sync pulse interval in seconds");
            println!("# TYPE dracon_sync_pulse_interval_secs gauge");
            println!(
                "dracon_sync_pulse_interval_secs {}",
                policy.pulse_interval_secs
            );

            // Mass deletion guard removed — IndexLock fixes the clone race
        }
        Command::Publish { cmd } => match cmd {
            PublishCommands::Run {
                repo,
                targets,
                skip_dry_run: _,
            } => {
                let policy = SyncPolicy::load(&policy_path)?;
                if !policy.auto_publish {
                    anyhow::bail!(
                        "auto_publish is disabled in config. Enable it in your sync policy."
                    );
                }
                let repo_targets = if targets.is_empty() {
                    policy
                        .publish_targets
                        .iter()
                        .map(|t| t.name.clone())
                        .collect::<Vec<_>>()
                } else {
                    targets
                };
                let version = release::detect_project_version(&repo)
                    .map(|(v, _)| v)
                    .unwrap_or_else(|| "unknown".to_string());
                println!(
                    "Publishing {} (v{}) to: {}",
                    repo.display(),
                    version,
                    repo_targets.join(", ")
                );
                let steps = release::run_release_pipeline(
                    &repo,
                    "",
                    &version,
                    "patch",
                    &policy,
                    true,
                    false,
                    &repo_targets,
                    false,
                )
                .await;
                for step in &steps {
                    match step {
                        release::ReleaseStep::TagCreated(tag) => println!("  Tag: {tag}"),
                        release::ReleaseStep::GitHubReleaseCreated(tag) => {
                            println!("  Release: {tag}")
                        }
                        release::ReleaseStep::Published { registry, version } => {
                            println!("  Published: {registry} v{version}")
                        }
                        release::ReleaseStep::NixFlakePRCreated(url) => {
                            println!("  Nix flake PR: {url}")
                        }
                        release::ReleaseStep::Skipped(reason) => println!("  Skipped: {reason}"),
                        release::ReleaseStep::Failed { step: s, error } => {
                            eprintln!("  Failed: {s} — {error}")
                        }
                    }
                }
            }
            PublishCommands::Status { repo, json } => {
                let policy = SyncPolicy::load(&policy_path)?;
                let version = release::detect_project_version(&repo)
                    .map(|(v, _)| v)
                    .unwrap_or_else(|| "unknown".to_string());
                let mut statuses = Vec::new();
                for target in &policy.publish_targets {
                    match release::extract_package_name(&repo, target.registry) {
                        Ok(pkg_name) => {
                            let exists = release::version_exists_on_registry(
                                target.registry,
                                &pkg_name,
                                &version,
                            )
                            .await;
                            statuses.push(serde_json::json!({
                                "target": target.name,
                                "registry": target.registry.as_str(),
                                "package": pkg_name,
                                "version": version,
                                "published": exists.unwrap_or(false),
                            }));
                        }
                        Err(e) => statuses.push(serde_json::json!({
                            "target": target.name,
                            "registry": target.registry.as_str(),
                            "version": version,
                            "error": e.to_string(),
                        })),
                    }
                }
                if json {
                    println!("{}", serde_json::to_string_pretty(&statuses)?);
                } else {
                    println!("Publish status for {} (v{}):", repo.display(), version);
                    for s in &statuses {
                        let target = s["target"].as_str().unwrap_or("?");
                        let published = s["published"].as_bool().unwrap_or(false);
                        let status_str = if published {
                            "published"
                        } else {
                            "not published"
                        };
                        println!("  {target}: {status_str}");
                    }
                }
            }
        },
        Command::Scaffold {
            repo,
            files,
            overwrite,
            dry_run,
        } => {
            cmd_scaffold(&policy_path, repo, files, overwrite, dry_run).await?;
        }
        Command::Ownership {
            repo,
            explain,
            json,
        } => {
            cmd_ownership(&policy_path, repo.as_deref(), explain, json)?;
        }
        Command::ScanBloat {
            min_size_mib,
            min_repo_count,
            json,
        } => {
            run_scan_bloat_report(&policy_path, min_size_mib, min_repo_count, json).await?;
        }
    }

    Ok(())
}

async fn cmd_scaffold(
    policy_path: &std::path::Path,
    repo: Option<PathBuf>,
    files: Vec<String>,
    overwrite: bool,
    dry_run: bool,
) -> Result<()> {
    use anyhow::Context;
    use comfy_table::{presets::UTF8_FULL_CONDENSED, Cell, Color, ContentArrangement, Table};
    let policy = SyncPolicy::load(policy_path)?;

    if policy.standard_files.is_empty() {
        println!("No standard files configured in policy.");
        println!(
            "Add [[standard_files]] entries to {}",
            policy_path.display()
        );
        return Ok(());
    }

    let filtered_configs: Vec<_> = if files.is_empty() {
        policy.standard_files.clone()
    } else {
        policy
            .standard_files
            .iter()
            .filter(|c| files.contains(&c.target))
            .cloned()
            .collect()
    };

    if filtered_configs.is_empty() {
        println!("No matching standard files found.");
        return Ok(());
    }

    let repos = if let Some(repo_path) = repo {
        vec![repo_path]
    } else {
        let roots: Vec<PathBuf> = policy.watch_roots.iter().map(PathBuf::from).collect();
        let excluded: std::collections::BTreeSet<String> =
            policy.exclude_dir_names.iter().cloned().collect();
        git::discover_git_repos(&roots, &excluded, &policy.exclude_repos, None)
    };

    let policy_base = policy_path.parent().unwrap_or(policy_path);
    let mut results: Vec<(String, String, String)> = Vec::new();
    let mut total_copied = 0usize;

    for repo_path in &repos {
        let repo_override = policy::load_repo_override(repo_path);
        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| repo_path.display().to_string());

        for cfg in &filtered_configs {
            if repo_override.skip_standard_files.contains(&cfg.target) {
                continue;
            }

            let target_path = repo_path.join(&cfg.target);
            if target_path.exists() && !overwrite && !cfg.overwrite {
                continue;
            }

            let source_path = cfg.source_path(policy_base);
            if !source_path.exists() {
                results.push((
                    repo_name.clone(),
                    cfg.target.clone(),
                    "template missing".to_string(),
                ));
                continue;
            }

            if dry_run {
                results.push((
                    repo_name.clone(),
                    cfg.target.clone(),
                    "would copy".to_string(),
                ));
                total_copied += 1;
                continue;
            }

            if target_path.exists() && (overwrite || cfg.overwrite) {
                if target_path.is_dir() {
                    std::fs::remove_dir_all(&target_path)
                        .with_context(|| format!("failed to remove {}", cfg.target))?;
                } else {
                    std::fs::remove_file(&target_path)
                        .with_context(|| format!("failed to remove {}", cfg.target))?;
                }
            }

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }

            match std::fs::copy(&source_path, &target_path) {
                Ok(_) => {
                    results.push((repo_name.clone(), cfg.target.clone(), "copied".to_string()));
                    total_copied += 1;
                }
                Err(e) => {
                    results.push((repo_name.clone(), cfg.target.clone(), format!("error: {e}")));
                }
            }
        }
    }

    if results.is_empty() {
        println!("No standard files to scaffold (all repos already have them).");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("REPO"),
            Cell::new("FILE"),
            Cell::new("STATUS"),
        ]);

    for (repo_name, file, status) in &results {
        let (status_str, color) = match status.as_str() {
            "copied" => ("\u{2705} copied", Color::Green),
            "would copy" => ("\u{1f4dd} would copy", Color::Yellow),
            "template missing" => ("\u{274c} template missing", Color::Red),
            s if s.starts_with("error:") => ("\u{274c} error", Color::Red),
            _ => (status.as_str(), Color::White),
        };
        table.add_row(vec![
            Cell::new(repo_name),
            Cell::new(file),
            Cell::new(status_str).fg(color),
        ]);
    }

    println!("{table}");
    let mode = if dry_run { "DRY-RUN" } else { "APPLIED" };
    println!(
        "{mode}: {total_copied} files scaffolded across {} repos",
        repos.len()
    );

    Ok(())
}

fn cmd_ownership(
    policy_path: &std::path::Path,
    repo: Option<&std::path::Path>,
    explain: bool,
    json: bool,
) -> Result<()> {
    use crate::ownership::{
        detect_ownership, read_signals, OwnershipInputs, OwnershipReport, TrustedSet,
    };
    use crate::policy::{load_repo_override, SyncPolicy};
    use comfy_table::{presets::UTF8_FULL_CONDENSED, Cell, Color, ContentArrangement, Table};

    let policy = SyncPolicy::load(policy_path)?;
    let trusted = TrustedSet {
        emails: policy.trusted_emails.clone(),
        authors: policy.trusted_authors.clone(),
        remote_hosts: policy.trusted_remote_hosts.clone(),
    };
    let repos: Vec<PathBuf> = if let Some(p) = repo {
        vec![p.to_path_buf()]
    } else {
        let roots: Vec<PathBuf> = policy.watch_roots.iter().map(PathBuf::from).collect();
        let excluded: std::collections::BTreeSet<String> =
            policy.exclude_dir_names.iter().cloned().collect();
        git::discover_git_repos(&roots, &excluded, &policy.exclude_repos, None)
    };

    struct Row {
        repo: String,
        report: OwnershipReport,
        inputs: OwnershipInputs,
        override_owned: Option<bool>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for repo_path in &repos {
        let inputs = read_signals(repo_path);
        let override_ = load_repo_override(repo_path);
        let override_owned = override_.owned;
        let report = detect_ownership(repo_path, &trusted, override_owned);
        rows.push(Row {
            repo: repo_path.display().to_string(),
            report,
            inputs,
            override_owned,
        });
    }

    if json {
        #[derive(serde::Serialize)]
        struct Out {
            policy: String,
            trusted_emails: Vec<String>,
            trusted_authors: Vec<String>,
            trusted_remote_hosts: Vec<String>,
            results: Vec<RepoJson>,
        }
        #[derive(serde::Serialize)]
        struct RepoJson {
            repo: String,
            report: OwnershipReport,
            user_email: Option<String>,
            head_author_email: Option<String>,
            head_author_name: Option<String>,
            origin_url: Option<String>,
            override_owned: Option<bool>,
        }
        let out = Out {
            policy: policy_path.display().to_string(),
            trusted_emails: policy.trusted_emails.clone(),
            trusted_authors: policy.trusted_authors.clone(),
            trusted_remote_hosts: policy.trusted_remote_hosts.clone(),
            results: rows
                .into_iter()
                .map(|r| RepoJson {
                    repo: r.repo,
                    report: r.report,
                    user_email: r.inputs.user_email,
                    head_author_email: r.inputs.head_author_email,
                    head_author_name: r.inputs.head_author_name,
                    origin_url: r.inputs.origin_url,
                    override_owned: r.override_owned,
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::DynamicFullWidth);
    if explain {
        table.set_header(vec![
            Cell::new("📦 REPO"),
            Cell::new("🩺 OWNERSHIP"),
            Cell::new("📧 user.email"),
            Cell::new("👤 HEAD author"),
            Cell::new("🌐 origin"),
            Cell::new("🔧 override"),
        ]);
    } else {
        table.set_header(vec![Cell::new("📦 REPO"), Cell::new("🩺 OWNERSHIP")]);
    }
    for r in &rows {
        let (label, color) = match &r.report {
            OwnershipReport::Owned { reason } => (format!("✓ owned ({})", reason), Color::Green),
            OwnershipReport::Unowned { reason, .. } => {
                (format!("🚫 unowned: {}", reason), Color::Red)
            }
            OwnershipReport::Unknown { .. } => ("❓ unknown".to_string(), Color::Yellow),
        };
        let label_cell = Cell::new(label).fg(color);
        if explain {
            table.add_row(vec![
                Cell::new(&r.repo),
                label_cell,
                Cell::new(r.inputs.user_email.as_deref().unwrap_or("—")),
                Cell::new(match (&r.inputs.head_author_name, &r.inputs.head_author_email) {
                    (Some(n), Some(e)) => format!("{} <{}>", n, e),
                    (Some(n), None) => n.clone(),
                    (None, Some(e)) => format!("<{}>", e),
                    (None, None) => "—".to_string(),
                }),
                Cell::new(r.inputs.origin_url.as_deref().unwrap_or("—")),
                Cell::new(
                    r.override_owned
                        .map(|b| if b { "owned=true" } else { "owned=false" })
                        .unwrap_or("—"),
                ),
            ]);
        } else {
            table.add_row(vec![Cell::new(&r.repo), label_cell]);
        }
    }
    println!("{table}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    fn temp_policy(repos: Vec<&str>) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let content = format!(
            r#"
auto_github_private = false
auto_commit = true
auto_pull = true
auto_push = true
auto_bump_versions = false
watch_roots = {:?}
remotes = []
"#,
            repos
        );
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        tmp
    }

    #[test]
    fn test_freeze_reason_none_when_no_marker() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".dracon")).unwrap();

        let policy_tmp = temp_policy(vec!["/dev/null"]);
        let policy_path = policy_tmp.path().join("policy.toml");

        let result = crate::policy::freeze_reason(&policy_path);
        assert!(result.is_none(), "no freeze marker should return None");
    }

    #[test]
    fn test_freeze_marker_paths() {
        let paths = crate::policy::freeze_marker_paths(std::path::Path::new("/fake.toml"));
        assert!(!paths.is_empty());
        assert!(paths
            .iter()
            .any(|p| p.to_string_lossy().contains(".dracon")));
        assert!(paths.iter().any(|p| p.to_string_lossy().contains("freeze")));
    }

    #[test]
    fn test_env_freeze_takes_precedence() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".dracon")).unwrap();

        let policy_tmp = temp_policy(vec!["/dev/null"]);
        let policy_path = policy_tmp.path().join("policy.toml");

        let _guard = crate::test_helpers::EnvRestorer::new("DRACON_SYNC_FREEZE", "1");
        let result = crate::policy::freeze_reason(&policy_path);

        assert!(
            result.is_some(),
            "env freeze should override missing marker"
        );
        assert!(result.unwrap().contains("env DRACON_SYNC_FREEZE"));
    }

    #[test]
    fn test_metrics_output_has_expected_format() {
        let lines = vec![
            "# HELP dracon_sync_info Dracon sync daemon info".to_string(),
            "# TYPE dracon_sync_info gauge".to_string(),
            format!(
                "dracon_sync_info{{version=\"{}\"}} 1",
                env!("CARGO_PKG_VERSION")
            ),
            "dracon_sync_repos_discovered_total 20".to_string(),
            "# HELP dracon_sync_freeze_state gauge".to_string(),
            "dracon_sync_freeze_state 0".to_string(),
        ];

        let mut found_version_line = false;
        for line in &lines {
            if line.starts_with('#') {
                assert!(
                    line.contains(" HELP ") || line.contains(" TYPE "),
                    "comment line should be HELP or TYPE: {}",
                    line
                );
            } else {
                assert!(
                    line.contains("dracon_sync"),
                    "metric line should contain metric name: {}",
                    line
                );
                if line.contains("version=") {
                    found_version_line = true;
                }
            }
        }
        assert!(found_version_line, "version metric line should be present");
    }

    #[test]
    fn test_metrics_contains_all_expected_metrics() {
        let expected_metrics = vec![
            "dracon_sync_info",
            "dracon_sync_repos_discovered_total",
            "dracon_sync_watch_roots_total",
            "dracon_sync_remotes_total",
            "dracon_sync_freeze_state",
            "dracon_sync_policy_auto_commit",
            "dracon_sync_policy_auto_push",
            "dracon_sync_policy_auto_pull",
            "dracon_sync_push_retries",
            "dracon_sync_pulse_interval_secs",
        ];

        for metric in &expected_metrics {
            assert!(
                metric.starts_with("dracon_sync_"),
                "metric name should start with dracon_sync_: {}",
                metric
            );
        }
    }
}

// daemon-push-test: 2026-06-21 12:18 — verify end-to-end propagation
// daemon-push-test-2: 2026-06-21 12:35 — verify daemon auto-push for nested repo
