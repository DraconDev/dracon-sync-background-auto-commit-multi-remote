//! Daemon-managed git hooks enforcing the fleet's no-history-rewrite
//! policy (added 2026-07-25, v0.113.0).
//!
//! ## Why
//!
//! The 2026-07-25 incident (hegemon filter-branch churn, virtual-pet
//! amend loop, pully rebase) showed that agent loops will rewrite
//! already-pushed history — and every rewrite races the daemon's
//! auto-push, producing divergent-branch CONCERNs. AGENTS.md policy
//! files are a *soft* guard (agents may not read them) and gitlab
//! branch protection only covers gitlab (GitHub free-tier private
//! repos cannot be protected server-side). These hooks are the
//! *hard*, forge-independent enforcement: a non-fast-forward push or
//! a rebase of pushed commits is refused LOCALLY, before any forge
//! is involved.
//!
//! ## What is installed
//!
//! - `pre-push`: refuses any ref update where the remote tip is not
//!   an ancestor of the pushed tip (non-fast-forward), and refuses
//!   branch deletions. New branches (remote sha all-zeros) are
//!   allowed. Amending an *unpushed* commit still pushes fine
//!   (fast-forward), so normal WIP workflow is unaffected.
//! - `pre-rebase`: refuses a rebase when any commit in the rebased
//!   range is already contained in a remote-tracking branch (i.e.
//!   would rewrite published history). Rebasing unpushed local work
//!   (including `git pull --rebase` of unpushed commits) is allowed.
//!
//! ## Escape hatch
//!
//! `DRACON_ALLOW_REWRITE=1 git push --force ...` (or rebase) bypasses
//! both hooks — the operator keeps full control, deliberately and
//! audibly.
//!
//! ## Management semantics
//!
//! - Installed into `<gitdir>/hooks/` (resolved via
//!   `git rev-parse --git-path hooks`, so worktrees/submodules whose
//!   `.git` is a file work correctly).
//! - Our scripts carry `# dracon-sync-managed`; files containing the
//!   marker are rewritten in place when the embedded script changes
//!   (upgrade path).
//! - A pre-existing foreign hook is preserved by renaming it to
//!   `<name>.pre-dracon`; our script execs it first and honors its
//!   exit code.
//! - `ensure_no_rewrite_hooks` is called once per repo at daemon
//!   startup and on every `sync_repo` pass — both calls are cheap
//!   (two file stats in the steady state).

use std::path::{Path, PathBuf};

const MARKER: &str = "# dracon-sync-managed";

const PRE_PUSH: &str = r#"#!/bin/sh
# dracon-sync-managed — history-rewrite guard (v0.113.0)
# Refuse non-fast-forward pushes and branch deletions. An amended or
# rebased commit that was already pushed can never be fast-forward,
# so this blocks history rewrites while leaving normal pushes (and
# amends of UNPUSHED commits) untouched.
# Bypass deliberately: DRACON_ALLOW_REWRITE=1 git push --force ...
if [ -n "$DRACON_ALLOW_REWRITE" ]; then exit 0; fi

# Preserve stdin for our own ff-checks after any chained hook runs.
tmp="$(mktemp 2>/dev/null || echo /tmp/dracon-pre-push.$$)"
cat > "$tmp"

# Chain a pre-existing hook saved by the installer; honor its verdict.
if [ -x "$0.pre-dracon" ]; then
    "$0.pre-dracon" "$@" < "$tmp"
    rc=$?
    if [ $rc -ne 0 ]; then rm -f "$tmp"; exit $rc; fi
fi

while read -r local_ref local_sha remote_ref remote_sha; do
    # New ref on the remote: nothing to rewrite.
    if [ "$remote_sha" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi
    # Branch deletion.
    if [ "$local_sha" = "0000000000000000000000000000000000000000" ]; then
        echo "dracon-sync: refusing to delete $remote_ref (history guard)." >&2
        echo "  Bypass: DRACON_ALLOW_REWRITE=1" >&2
        rm -f "$tmp"; exit 1
    fi
    # Non-fast-forward: the remote tip is not an ancestor of ours.
    if ! git merge-base --is-ancestor "$remote_sha" "$local_sha" 2>/dev/null; then
        echo "dracon-sync: refusing non-fast-forward push to $remote_ref (history rewrite)." >&2
        echo "  Merge instead: git pull --no-rebase" >&2
        echo "  Bypass: DRACON_ALLOW_REWRITE=1" >&2
        rm -f "$tmp"; exit 1
    fi
done < "$tmp"
rm -f "$tmp"
exit 0
"#;

const PRE_REBASE: &str = r#"#!/bin/sh
# dracon-sync-managed — history-rewrite guard (v0.113.0)
# Refuse rebases that would rewrite commits already published to any
# remote. Rebasing unpushed local work is fine (including
# 'git pull --rebase' of commits the daemon has not pushed yet).
# Bypass deliberately: DRACON_ALLOW_REWRITE=1 git rebase ...
if [ -n "$DRACON_ALLOW_REWRITE" ]; then exit 0; fi

# Chain a pre-existing hook saved by the installer; honor its verdict.
if [ -x "$0.pre-dracon" ]; then
    "$0.pre-dracon" "$@" || exit $?
fi

upstream="$1"
[ -z "$upstream" ] && exit 0

for c in $(git rev-list "$upstream"..HEAD 2>/dev/null | head -100); do
    if [ -n "$(git branch -r --contains "$c" 2>/dev/null)" ]; then
        echo "dracon-sync: refusing rebase — $c is already published on a remote." >&2
        echo "  Rebasing it would rewrite pushed history and diverge the fleet mirrors." >&2
        echo "  Merge instead: git pull --no-rebase" >&2
        echo "  Bypass: DRACON_ALLOW_REWRITE=1" >&2
        exit 1
    fi
done
exit 0
"#;

/// What the ensure pass did for one hook file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookAction {
    /// Freshly installed (no prior file).
    Installed,
    /// Our marker was present but the content was stale → rewritten.
    Updated,
    /// Foreign hook existed → preserved as `<name>.pre-dracon`, ours
    /// installed in its place.
    ChainedForeign,
    /// Already installed and current — no filesystem writes.
    Current,
}

/// Resolve the hooks dir for a repo, handling worktrees/submodules
/// (whose `.git` is a file). Returns None if git can't resolve it.
fn hooks_dir(repo: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--git-path", "hooks"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() {
        return None;
    }
    let path = PathBuf::from(&p);
    // --git-path may return a repo-relative path (".git/hooks").
    Some(if path.is_absolute() {
        path
    } else {
        repo.join(path)
    })
}

/// Install/update one managed hook, preserving any foreign hook.
fn ensure_one(hooks: &Path, name: &str, content: &str) -> std::io::Result<HookAction> {
    let target = hooks.join(name);
    if target.exists() {
        let existing = std::fs::read_to_string(&target).unwrap_or_default();
        if existing.contains(MARKER) {
            if existing == content {
                return Ok(HookAction::Current);
            }
            std::fs::write(&target, content)?;
            make_executable(&target);
            return Ok(HookAction::Updated);
        }
        // Foreign hook: preserve it as <name>.pre-dracon, then install
        // ours. If a .pre-dracon already exists (previously preserved),
        // the new foreign file replaces it — the newest operator intent
        // wins, and the overwrite is logged by the caller.
        let saved = hooks.join(format!("{}.pre-dracon", name));
        std::fs::rename(&target, &saved)?;
        make_executable(&saved);
        std::fs::write(&target, content)?;
        make_executable(&target);
        return Ok(HookAction::ChainedForeign);
    }
    std::fs::write(&target, content)?;
    make_executable(&target);
    Ok(HookAction::Installed)
}

#[cfg(unix)]
fn make_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(p) {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(p, perms);
    }
}

#[cfg(not(unix))]
fn make_executable(_p: &Path) {}

/// Install (or refresh) the no-history-rewrite hooks for one repo.
/// Idempotent and cheap in the steady state (two stats). Errors are
/// logged and swallowed — a hooks failure must never block a sync.
pub(crate) fn ensure_no_rewrite_hooks(repo: &Path) {
    let Some(dir) = hooks_dir(repo) else {
        return;
    };
    for (name, content) in [("pre-push", PRE_PUSH), ("pre-rebase", PRE_REBASE)] {
        match ensure_one(&dir, name, content) {
            Ok(HookAction::Current) => {}
            Ok(action) => {
                eprintln!("🪝 {} {} hook in {}", label(action), name, repo.display());
            }
            Err(e) => {
                eprintln!("⚠️ could not install {} hook in {}: {}", name, repo.display(), e);
            }
        }
    }
}

fn label(a: HookAction) -> &'static str {
    match a {
        HookAction::Installed => "installed",
        HookAction::Updated => "updated",
        HookAction::ChainedForeign => "chained foreign hook + installed",
        HookAction::Current => "current",
    }
}

/// Parse `size-garbage` (KiB) from `git count-objects -v` output.
/// Returns BYTES (the count-objects values are KiB — the v0.112.42
/// unit lesson).
pub(crate) fn parse_count_objects_garbage_bytes(stdout: &str) -> u64 {
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("size-garbage:") {
            return rest.trim().parse::<u64>().unwrap_or(0) * 1024;
        }
    }
    0
}

/// Run `git gc --prune=now` when the repo's dangling-garbage size
/// exceeds `threshold_bytes`. Returns Some(garbage_bytes) when a gc
/// ran. Best-effort: all failures are logged, never fatal.
///
/// Motivation: hegemon's `.git` ballooned to 4.9 GiB and
/// dracon-platform's to 37 GiB from dangling tmp_pack_* objects
/// (failed/interrupted pushes), tripping the 2 GiB GitHub pack guard
/// and disk pressure. Manual `git gc --prune=now` fixed both; this
/// knob makes the daemon self-heal instead of waiting for the next
/// disk-pressure incident. `threshold_bytes = 0` disables.
pub(crate) fn maybe_auto_gc(repo: &Path, threshold_bytes: u64) -> Option<u64> {
    if threshold_bytes == 0 {
        return None;
    }
    let out = std::process::Command::new("git")
        .args(["count-objects", "-v"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let garbage = parse_count_objects_garbage_bytes(&String::from_utf8_lossy(&out.stdout));
    if garbage < threshold_bytes {
        return None;
    }
    eprintln!(
        "🗑️ {} has {:.2} GiB dangling garbage (> threshold {:.2} GiB) — running git gc --prune=now",
        repo.display(),
        garbage as f64 / 1073741824.0,
        threshold_bytes as f64 / 1073741824.0,
    );
    let started = std::time::Instant::now();
    match std::process::Command::new("git")
        .args(["gc", "--prune=now", "--quiet"])
        .current_dir(repo)
        .output()
    {
        Ok(o) if o.status.success() => {
            eprintln!(
                "🗑️ gc done for {} in {:.1}s (reclaimed ~{:.2} GiB garbage)",
                repo.display(),
                started.elapsed().as_secs_f64(),
                garbage as f64 / 1073741824.0,
            );
        }
        Ok(o) => {
            eprintln!(
                "⚠️ gc failed for {}: {}",
                repo.display(),
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Err(e) => {
            eprintln!("⚠️ gc spawn failed for {}: {}", repo.display(), e);
        }
    }
    Some(garbage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        // Hermeticity: the operator's machine sets a GLOBAL
        // core.hooksPath (~/.config/git/hooks — the warden hooks).
        // `git rev-parse --git-path hooks` honors it, so without this
        // override the tests would install into the operator's real
        // global hooks dir. A repo-local override wins over the global
        // config and keeps every test inside its tempdir.
        std::process::Command::new("git")
            .args([
                "config",
                "core.hooksPath",
                tmp.path().join(".git/hooks").to_str().unwrap(),
            ])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        tmp
    }

    #[test]
    fn installs_both_hooks_fresh() {
        let tmp = init_repo();
        ensure_no_rewrite_hooks(tmp.path());
        let hooks = tmp.path().join(".git/hooks");
        let pre_push = std::fs::read_to_string(hooks.join("pre-push")).unwrap();
        let pre_rebase = std::fs::read_to_string(hooks.join("pre-rebase")).unwrap();
        assert!(pre_push.contains(MARKER));
        assert!(pre_rebase.contains(MARKER));
        assert!(pre_push.contains("merge-base --is-ancestor"));
        assert!(pre_rebase.contains("branch -r --contains"));
        // Executable bits set.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(hooks.join("pre-push")).unwrap().permissions().mode() & 0o111,
                0o111
            );
        }
    }

    #[test]
    fn ensure_is_idempotent() {
        let tmp = init_repo();
        ensure_no_rewrite_hooks(tmp.path());
        let hooks = tmp.path().join(".git/hooks");
        let before = std::fs::read_to_string(hooks.join("pre-push")).unwrap();
        ensure_no_rewrite_hooks(tmp.path());
        let after = std::fs::read_to_string(hooks.join("pre-push")).unwrap();
        assert_eq!(before, after);
        // No chained file created for our own hook.
        assert!(!hooks.join("pre-push.pre-dracon").exists());
    }

    #[test]
    fn preserves_foreign_hook_via_chaining() {
        let tmp = init_repo();
        let hooks = tmp.path().join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("pre-push"), "#!/bin/sh\necho foreign\n").unwrap();
        ensure_no_rewrite_hooks(tmp.path());
        let saved = std::fs::read_to_string(hooks.join("pre-push.pre-dracon")).unwrap();
        assert!(saved.contains("foreign"));
        let ours = std::fs::read_to_string(hooks.join("pre-push")).unwrap();
        assert!(ours.contains(MARKER));
        assert!(ours.contains("pre-dracon"));
        // Second ensure: foreign stays preserved, ours stays current.
        ensure_no_rewrite_hooks(tmp.path());
        let saved2 = std::fs::read_to_string(hooks.join("pre-push.pre-dracon")).unwrap();
        assert!(saved2.contains("foreign"));
    }

    #[test]
    fn updates_stale_managed_hook() {
        let tmp = init_repo();
        let hooks = tmp.path().join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(
            hooks.join("pre-push"),
            "#!/bin/sh\n# dracon-sync-managed\n# old version\n",
        )
        .unwrap();
        ensure_no_rewrite_hooks(tmp.path());
        let ours = std::fs::read_to_string(hooks.join("pre-push")).unwrap();
        assert!(ours.contains(MARKER));
        assert!(!ours.contains("old version"));
        assert!(!hooks.join("pre-push.pre-dracon").exists());
    }

    #[test]
    fn pre_push_hook_blocks_non_ff_and_allows_ff() {
        // Build two clones sharing a "remote" and verify the hook's
        // ff logic end-to-end via real git pushes.
        let remote = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .current_dir(remote.path())
            .output()
            .unwrap();
        let tmp = init_repo();
        std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "commit", "--allow-empty", "-m", "c1"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["remote", "add", "origin", remote.path().to_str().unwrap()])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        ensure_no_rewrite_hooks(tmp.path());
        // ff push: allowed.
        let ok = std::process::Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(ok.status.success(), "ff push should pass: {}", String::from_utf8_lossy(&ok.stderr));
        // Amend the pushed commit → non-ff push: blocked by our hook.
        std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "commit", "--amend", "--no-edit"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let blocked = std::process::Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(!blocked.status.success(), "non-ff push must be refused");
        assert!(String::from_utf8_lossy(&blocked.stderr).contains("non-fast-forward"));
        // Escape hatch: DRACON_ALLOW_REWRITE=1 allows the same push.
        let bypass = std::process::Command::new("git")
            .args(["push", "--force", "origin", "main"])
            .current_dir(tmp.path())
            .env("DRACON_ALLOW_REWRITE", "1")
            .output()
            .unwrap();
        assert!(bypass.status.success(), "escape hatch must work: {}", String::from_utf8_lossy(&bypass.stderr));
    }

    #[test]
    fn garbage_parse_kib_to_bytes() {
        let out = "count: 0\nsize: 0\nin-pack: 10\npacks: 1\nsize-pack: 64\nprune-packable: 0\ngarbage: 1\nsize-garbage: 2048\n";
        assert_eq!(parse_count_objects_garbage_bytes(out), 2048 * 1024);
        assert_eq!(parse_count_objects_garbage_bytes("count: 0\n"), 0);
    }

    #[test]
    fn auto_gc_disabled_at_zero_threshold() {
        let tmp = init_repo();
        assert_eq!(maybe_auto_gc(tmp.path(), 0), None);
    }
}
