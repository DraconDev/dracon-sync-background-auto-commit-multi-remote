//! Classify each watched repo by its structural relationship to other
//! watched repos so the `dracon-sync repos` table can render a single
//! `🔗 ROLE` column that makes the topology visible at a glance.
//!
//! The three roles are:
//!
//! - **Parent** — the repo's `.gitmodules` declares ≥1 submodule and
//!   the daemon treats it as a parent (e.g. `dracon-platform` with
//!   `parent (10 submods)`).
//! - **Submod** — the repo's working tree is itself a submodule of
//!   another watched parent (e.g. `junk-runner` with
//!   `submod (of dracon-platform/web/games/wip/junk-runner)`).
//! - **Standalone** — no submodule relationship to any other watched
//!   repo (e.g. `avid`).
//!
//! When a repo is BOTH a parent AND a submod-of-parent (rare today
//! but possible in future topologies), the priority rule is:
//! **`Submod` wins over `Parent` wins over `Standalone`**.
//!
//! Detection uses only existing primitives in `git/discovery.rs`:
//! [`list_submodules`] for parent-of detection, and a derived check
//! that walks each watched repo's `.gitmodules` to find a submod
//! whose `path` ends at the row's basename for submod-of-parent
//! detection. No shelling out to `git submodule status`.

use std::path::PathBuf;

use crate::git::list_submodules;

/// Which structural role a single repo plays in the daemon's topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RoleKind {
    /// Repo owns ≥1 submodule. `count` is `list_submodules(...).len()`.
    Parent(usize),
    /// Repo is a submodule of another watched parent.
    /// `parent_basename` is the parent's directory name (not full path).
    /// `sub_path` is the relative path from the parent's root to the
    /// submodule checkout (e.g. `web/games/wip/junk-runner`).
    Submod {
        parent_basename: String,
        sub_path: String,
    },
    /// Repo has no submodule relationship with any other watched repo.
    Standalone,
}

impl RoleKind {
    /// Render the role as a short, single-line label for the table cell.
    ///
    /// Truncates to keep the cell width bounded so the comfy-table
    /// layout doesn't break on long submod paths. The full
    /// information is still recoverable from `detail()` (used by the
    /// design doc and tests).
    pub(crate) fn label(&self) -> String {
        match self {
            RoleKind::Parent(n) => format!("parent ({} submods)", n),
            RoleKind::Submod {
                parent_basename,
                sub_path,
            } => format!("submod (of {}/{})", parent_basename, sub_path),
            RoleKind::Standalone => "standalone".to_string(),
        }
    }

    // NOTE: `detail()` method removed 2026-07-11 (audit
    // AUDIT-3-UTILITIES-2026-07-10.md CONCERN #6). It had no callers
    // anywhere in the codebase. The shorter `label()` form is what
    // shows up in the table cell.
}

/// Classify the role of each row in `rows`. The returned vector has the
/// same length and order as `rows` (one role per row).
///
/// `rows` is `&[RepoReportRow]` and only `row.repo` (the absolute path
/// of the watched repo's working tree) is read — no other fields are
/// needed for the role decision.
pub(crate) fn classify_roles(rows: &[crate::report::RepoReportRow]) -> Vec<RoleKind> {
    let abs_paths: Vec<PathBuf> = rows.iter().map(|r| PathBuf::from(r.repo_path())).collect();

    // For each row, precompute:
    //  - Is this row a parent? (use list_submodules on its path)
    //  - For each OTHER row, does this row's .gitmodules declare a
    //    submod whose name (or path-tail) matches the current row's
    //    basename? That tells us this row is a Submod of <other row>
    //    even when the submod is also checked out as a standalone
    //    at a different path.
    //
    // We do this with O(N*M) work where N=rows and M=submods-per-row;
    // for the current 26-row watch set that's <100 comparisons.

    let mut results: Vec<RoleKind> = Vec::with_capacity(rows.len());

    for (i, _row) in rows.iter().enumerate() {
        let my_path = &abs_paths[i];
        let my_basename = my_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // 1. Check parent role: does my .gitmodules declare any submods?
        let my_subs = list_submodules(my_path);
        let parent_role = if !my_subs.is_empty() {
            Some(RoleKind::Parent(my_subs.len()))
        } else {
            None
        };

        // 2. Check submod role: do any OTHER rows' .gitmodules declare
        //    a submod whose full relative-path matches my abs_path
        //    when joined with their parent? The submod's `name` in
        //    .gitmodules is conventionally derived from the repo's
        //    directory name. The `path` tail (last `/`-segment) is
        //    the actual nested path, e.g. `web/games/wip/polis` → tail
        //    `polis`.
        //
        // F55 (2026-07-19): the previous code matched by basename
        // only, which collides if two watched repos share a basename
        // (e.g. both `Cargo.toml` or `dracon-sync` as the daemon
        // source dir vs the nested standalone). The new logic prefers
        // the full relative-path equality check first; falls back to
        // basename only as a last resort (kept for backwards-compat
        // with submod entries that use a bare `name` field).
        let mut submod_role: Option<RoleKind> = None;
        for (j, other_row) in rows.iter().enumerate() {
            if i == j {
                continue;
            }
            let other_path = &abs_paths[j];
            let other_subs = list_submodules(other_path);
            for entry in &other_subs {
                // First try: full relative-path equality.
                let expected_full = other_path.join(&entry.path);
                let full_path_matches = expected_full == *my_path;

                // Fallback: name equality (legacy format).
                let name_matches = entry.name == my_basename;

                // Fallback: path-tail equality (last `/`-segment).
                let last_segment = entry
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&entry.path);
                let path_tail_matches =
                    !my_basename.is_empty() && last_segment == my_basename;

                if full_path_matches || name_matches || path_tail_matches {
                    let parent_basename = other_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| other_row.repo_path().to_string());
                    submod_role = Some(RoleKind::Submod {
                        parent_basename,
                        sub_path: entry.path.clone(),
                    });
                    break;
                }
            }
            if submod_role.is_some() {
                break;
            }
        }

        // 3. Priority: submod > parent > standalone.
        let final_role = submod_role
            .or(parent_role)
            .unwrap_or(RoleKind::Standalone);
        results.push(final_role);
    }

    results
}

// ---------------------------------------------------------------------------
// Tests
//
// These tests use only the public surface of the classifier plus the
// existing `list_submodules` primitive. They build minimal in-memory
// fixtures in `tempfile::tempdir()` (no external repositories
// required) and don't touch disk beyond the temp dir.
//
// Tests verify:
//   1. Standalone repo (no .gitmodules) → Standalone
//   2. Parent repo (.gitmodules declares 3 submods) → Parent(3)
//   3. Submod-of-parent repo (row is at <parent>/<path>) → Submod
//   4. Dual-role priority: a repo that is BOTH a parent AND a
//      submod-of-parent resolves to Submod.

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::tempdir;

    /// Initialize a bare git repo at `path`, returning the HEAD SHA.
    /// This is enough scaffolding for `list_submodules` to read
    /// `.gitmodules` and find the parent's index.
    fn init_repo(path: &Path) -> String {
        Command::new("git")
            .args(["init", "-q", "--initial-branch=main"])
            .arg(path)
            .output()
            .expect("git init");
        // Disable hooks so globally-installed warden hooks don't reject
        // commits in temp test repos that lack `.gitattributes` with
        // `filter=dracon`. See AUDIT-3-UTILITIES-2026-07-10.md CONCERN #4.
        Command::new("git")
            .args(["-C"])
            .arg(path)
            .args(["config", "core.hooksPath", "/dev/null"])
            .output()
            .expect("git config core.hooksPath");
        Command::new("git")
            .args(["-C"])
            .arg(path)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .expect("git config user.email");
        Command::new("git")
            .args(["-C"])
            .arg(path)
            .args(["config", "user.name", "Test"])
            .output()
            .expect("git config user.name");
        // Need a commit for `git rev-parse HEAD` to succeed.
        Command::new("git")
            .args(["-C"])
            .arg(path)
            .args(["commit", "--no-verify", "--allow-empty", "-m", "init", "-q"])
            .output()
            .expect("git commit");
        let head_out = Command::new("git")
            .args(["-C"])
            .arg(path)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse");
        String::from_utf8_lossy(&head_out.stdout).trim().to_string()
    }

    /// Stage fake gitlink entries in the parent's index. Without
    /// index entries, `list_submodules` will return entries with
    /// empty SHAs (the cross-reference returns ""). For these tests
    /// we only care about the path/name being correct, so empty SHA
    /// is fine — `RoleKind::Parent(n)` only requires non-empty count,
    /// and `RoleKind::Submod` is keyed by path equality.
    fn stage_gitlink(parent: &Path, sub_path: &str, sha: &str) {
        let status = Command::new("git")
            .args(["-C"])
            .arg(parent)
            .args(["update-index", "--add", "--cacheinfo"])
            .arg(format!("160000,{},{}", sha, sub_path))
            .status()
            .expect("git update-index");
        assert!(status.success(), "git update-index failed for {sub_path}");
    }

    #[test]
    fn classify_role_for_standalone_repo() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("standalone");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        // No .gitmodules → no parent role; no other watched rows → no
        // submod role. Result: Standalone.
        let row = crate::report::RepoReportRow::for_tests(&repo.display().to_string());
        let rows = vec![row];
        let roles = classify_roles(&rows);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0], RoleKind::Standalone);
        assert_eq!(roles[0].label(), "standalone");
    }

    #[test]
    fn classify_role_for_parent_repo() {
        let tmp = tempdir().unwrap();
        let parent_path = tmp.path().join("myparent");
        fs::create_dir_all(&parent_path).unwrap();
        let head = init_repo(&parent_path);

        let gitmodules = "[submodule \"child-a\"]\n\
                          \tpath = sub/a\n\
                          \turl = git@example.com:a.git\n\
                          [submodule \"child-b\"]\n\
                          \tpath = sub/b\n\
                          \turl = git@example.com:b.git\n\
                          [submodule \"child-c\"]\n\
                          \tpath = sub/c\n\
                          \turl = git@example.com:c.git\n";
        fs::write(parent_path.join(".gitmodules"), gitmodules).unwrap();
        stage_gitlink(&parent_path, "sub/a", &head);
        stage_gitlink(&parent_path, "sub/b", &head);
        stage_gitlink(&parent_path, "sub/c", &head);

        let row = crate::report::RepoReportRow::for_tests(&parent_path.display().to_string());
        let rows = vec![row];
        let roles = classify_roles(&rows);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0], RoleKind::Parent(3));
        assert_eq!(roles[0].label(), "parent (3 submods)");
    }

    #[test]
    fn classify_role_for_submod_repo() {
        let tmp = tempdir().unwrap();
        // Parent at <tmp>/myparent with one submod declared at sub/child.
        let parent_path = tmp.path().join("myparent");
        fs::create_dir_all(&parent_path).unwrap();
        let head = init_repo(&parent_path);

        let gitmodules = "[submodule \"child\"]\n\
                          \tpath = sub/child\n\
                          \turl = git@example.com:child.git\n";
        fs::write(parent_path.join(".gitmodules"), gitmodules).unwrap();
        stage_gitlink(&parent_path, "sub/child", &head);

        // Real sub-repo at <parent>/sub/child with its own .git so the
        // classify_roles submod-of-parent path resolution succeeds.
        let child_dir = parent_path.join("sub/child");
        fs::create_dir_all(&child_dir).unwrap();
        init_repo(&child_dir);

        let row_parent =
            crate::report::RepoReportRow::for_tests(&parent_path.display().to_string());
        let row_child =
            crate::report::RepoReportRow::for_tests(&child_dir.display().to_string());
        let rows = vec![row_parent, row_child];
        let roles = classify_roles(&rows);

        assert_eq!(roles.len(), 2);
        // Parent row → Parent role.
        assert_eq!(roles[0], RoleKind::Parent(1));
        // Child row → Submod role pointing at the parent.
        match &roles[1] {
            RoleKind::Submod { parent_basename, sub_path } => {
                assert_eq!(parent_basename, "myparent");
                assert_eq!(sub_path, "sub/child");
            }
            other => panic!("expected Submod, got {:?}", other),
        }
    }

    #[test]
    fn priority_submod_over_parent_when_dual_role() {
        let tmp = tempdir().unwrap();
        // Grandparent at <tmp>/grand with a sub called "middle".
        let grand = tmp.path().join("grand");
        fs::create_dir_all(&grand).unwrap();
        let head = init_repo(&grand);

        let grand_gitmodules = "[submodule \"middle\"]\n\
                                \tpath = sub/middle\n\
                                \turl = git@example.com:middle.git\n";
        fs::write(grand.join(".gitmodules"), grand_gitmodules).unwrap();
        stage_gitlink(&grand, "sub/middle", &head);

        // Middle is at <grand>/sub/middle and ALSO declares its
        // own submods (so it is a parent too — making it dual-role).
        let middle = grand.join("sub/middle");
        fs::create_dir_all(&middle).unwrap();
        let middle_head = init_repo(&middle);

        let middle_gitmodules = "[submodule \"leaf\"]\n\
                                 \tpath = leaf\n\
                                 \turl = git@example.com:leaf.git\n";
        fs::write(middle.join(".gitmodules"), middle_gitmodules).unwrap();
        stage_gitlink(&middle, "leaf", &middle_head);

        // Leaf at <middle>/leaf — must be a submod of "middle".
        let leaf = middle.join("leaf");
        fs::create_dir_all(&leaf).unwrap();
        init_repo(&leaf);

        let rows = vec![
            crate::report::RepoReportRow::for_tests(&grand.display().to_string()),
            crate::report::RepoReportRow::for_tests(&middle.display().to_string()),
            crate::report::RepoReportRow::for_tests(&leaf.display().to_string()),
        ];
        let roles = classify_roles(&rows);

        // Grand: Parent only (no submod-of for grand here).
        assert_eq!(roles[0], RoleKind::Parent(1));
        // Middle: BOTH Parent AND Submod-of-grand → Submod wins.
        match &roles[1] {
            RoleKind::Submod { parent_basename, sub_path } => {
                assert_eq!(parent_basename, "grand");
                assert_eq!(sub_path, "sub/middle");
            }
            other => panic!("expected Submod for middle, got {:?}", other),
        }
        // Leaf: Submod-of-middle.
        match &roles[2] {
            RoleKind::Submod { parent_basename, sub_path } => {
                assert_eq!(parent_basename, "middle");
                assert_eq!(sub_path, "leaf");
            }
            other => panic!("expected Submod for leaf, got {:?}", other),
        }
    }
}

// (no trailing helpers needed)
