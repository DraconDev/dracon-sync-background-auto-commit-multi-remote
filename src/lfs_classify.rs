//! Reviewed real-file classification adapter for the LFS routing evaluator.
//!
//! This adapter maps real filesystem snapshots to the typed [`Evidence`]
//! consumed by [`crate::lfs_policy::evaluate`]. It performs no mutations:
//! files are only stated and (for magic sniffing) read. Classification is
//! deliberately conservative: a path is an approved asset only when its
//! extension appears in the caller-supplied reviewed allow-list, while
//! sensitive and database indicators always win regardless of the allow-list.
//! Extensions alone never promote an unknown payload.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

use super::lfs_policy::{Classification, Evidence, Filter};

/// Caller-reviewed allow-list of asset extensions (lowercase, without dots).
#[derive(Clone, Debug)]
pub struct ReviewRules {
    /// Extensions reviewed as non-sensitive asset snapshots, e.g. `png`.
    pub asset_extensions: Vec<String>,
}

impl ReviewRules {
    /// Build rules from an explicit allow-list slice.
    pub fn new(asset_extensions: &[&str]) -> Self {
        Self {
            asset_extensions: asset_extensions
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect(),
        }
    }
}

/// Snapshot-scoped context supplied alongside classification.
#[derive(Clone, Copy, Debug)]
pub struct SnapshotContext {
    /// Measured own-repository history bytes.
    pub history_bytes: Option<u64>,
    /// Measured churn bytes for the candidate.
    pub churn_bytes: Option<u64>,
    /// Previously enrolled path; must survive size reductions.
    pub enrolled: bool,
    /// Operator-approved private binding, not an arbitrary repo URL.
    pub approved_binding: bool,
    /// Explicit per-repository opt-out.
    pub opted_out: bool,
    /// Ignore/generated-output rules take precedence.
    pub excluded: bool,
    /// Effective Git filter after attribute precedence is resolved.
    pub filter: Filter,
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn extension_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn starts_with_sqlite_magic(path: &Path) -> bool {
    let mut header = [0u8; 16];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    match file.read_exact(&mut header) {
        Ok(()) => header == *b"SQLite format 3\0",
        Err(_) => false,
    }
}

/// Classify a real file without mutating it.
///
/// Precedence: sensitive indicators, then database indicators (extension or
/// SQLite magic), then SQL source text, then the reviewed asset allow-list;
/// anything else is [`Classification::Unknown`].
pub fn classify(path: &Path, rules: &ReviewRules) -> io::Result<Classification> {
    // Metadata first so missing/unreadable paths surface as errors, not Unknown.
    let _ = fs::metadata(path)?;
    let name = file_name_lower(path);
    let ext = extension_lower(path);
    if matches!(ext.as_str(), "pem" | "key" | "age" | "env")
        || matches!(
            name.as_str(),
            ".env" | "id_rsa" | "id_ed25519" | "secrets.json"
        )
    {
        return Ok(Classification::Sensitive);
    }
    if matches!(
        ext.as_str(),
        "db" | "sqlite" | "sqlite3" | "sqlitedb" | "dump" | "wal" | "shm"
    ) || starts_with_sqlite_magic(path)
    {
        return Ok(Classification::Database);
    }
    if ext == "sql" {
        return Ok(Classification::SourceText);
    }
    if rules.asset_extensions.contains(&ext) {
        return Ok(Classification::ApprovedAsset);
    }
    Ok(Classification::Unknown)
}

/// Build evaluator evidence for a real file; `bytes` always equals the live
/// metadata length so size-driven tests assert against real measurements.
pub fn evidence_for(
    path: &Path,
    rules: &ReviewRules,
    context: SnapshotContext,
) -> io::Result<Evidence> {
    Ok(Evidence {
        classification: classify(path, rules)?,
        filter: context.filter,
        bytes: fs::metadata(path)?.len(),
        history_bytes: context.history_bytes,
        churn_bytes: context.churn_bytes,
        enrolled: context.enrolled,
        approved_binding: context.approved_binding,
        opted_out: context.opted_out,
        excluded: context.excluded,
    })
}

#[cfg(test)]
mod tests {
    use super::super::lfs_policy::{self, Decision, HoldReason, Policy, PolicyOverride};
    use super::*;
    use std::fs::File;
    use std::sync::atomic::{AtomicU64, Ordering};

    const MIB: u64 = 1024 * 1024;
    static FIXTURE_SEQ: AtomicU64 = AtomicU64::new(0);

    fn policy() -> Policy {
        Policy {
            enabled: true,
            minimum_asset_bytes: 20 * MIB,
            individual_trigger_bytes: 100 * MIB,
            history_trigger_bytes: 1024 * MIB,
            churn_trigger_bytes: 100 * MIB,
        }
    }

    fn rules() -> ReviewRules {
        ReviewRules::new(&["png", "jpg", "mp4", "bin"])
    }

    fn context(history_bytes: Option<u64>, churn_bytes: Option<u64>) -> SnapshotContext {
        SnapshotContext {
            history_bytes,
            churn_bytes,
            enrolled: false,
            approved_binding: true,
            opted_out: false,
            excluded: false,
            filter: Filter::Git,
        }
    }

    struct Fixture {
        root: std::path::PathBuf,
    }

    impl Fixture {
        /// Fresh unique tempdir per test; removed on drop.
        fn fresh() -> io::Result<Self> {
            let id = FIXTURE_SEQ.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir().join(format!(
                "dracon-lfs-classify-fixture-{}-{}",
                std::process::id(),
                id
            ));
            fs::create_dir_all(&root)?;
            Ok(Self { root })
        }

        /// Create a real directory entry with the given logical size.
        /// Sizes are sparse (unwritten ranges read as zeros), so the
        /// multi-hundred-MiB fixtures stay fast and disk-light while
        /// `metadata().len()` reports the true logical size under test.
        fn sized(&self, name: &str, bytes: u64) -> io::Result<std::path::PathBuf> {
            let path = self.root.join(name);
            let file = File::create(&path)?;
            file.set_len(bytes)?;
            Ok(path)
        }

        fn route(
            &self,
            path: &Path,
            history: Option<u64>,
            churn: Option<u64>,
        ) -> io::Result<Decision> {
            let evidence = evidence_for(path, &rules(), context(history, churn))?;
            assert_eq!(evidence.bytes, fs::metadata(path)?.len());
            Ok(lfs_policy::evaluate(Some(policy()), evidence))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn two_hundred_mib_small_image_repo_stays_in_git() -> io::Result<()> {
        // 200 x 1 MiB approved images: history pressure (200 MiB) stays far
        // below the 1 GiB trigger, so every file must keep ordinary Git.
        let fixture = Fixture::fresh()?;
        let mut paths = Vec::with_capacity(200);
        for i in 0..200 {
            paths.push(fixture.sized(&format!("asset-{i:03}.png"), MIB)?);
        }
        for path in &paths {
            assert_eq!(
                fixture.route(path, Some(200 * MIB), Some(0))?,
                Decision::KeepGit
            );
        }
        Ok(())
    }

    #[test]
    fn large_sensitive_key_is_refused_not_enrolled() -> io::Result<()> {
        let fixture = Fixture::fresh()?;
        let path = fixture.sized("backup-key.pem", 150 * MIB)?;
        assert_eq!(
            fixture.route(&path, Some(200 * MIB), Some(0))?,
            Decision::Hold(HoldReason::Classification)
        );
        Ok(())
    }

    #[test]
    fn large_database_is_refused_not_enrolled() -> io::Result<()> {
        let fixture = Fixture::fresh()?;
        let path = fixture.sized("snapshot.db", 150 * MIB)?;
        // Real SQLite magic at the head of an extensionless payload is also caught.
        // (Opened without truncate: `fs::write` would shrink the sparse file
        // back to the 16-byte header.)
        let magicless = fixture.sized("opaque-payload", 150 * MIB)?;
        {
            use std::io::Write;
            let mut handle = fs::OpenOptions::new().write(true).open(&magicless)?;
            handle.write_all(b"SQLite format 3\0")?;
        }
        for candidate in [&path, &magicless] {
            assert_eq!(classify(candidate, &rules())?, Classification::Database);
            assert_eq!(
                fixture.route(candidate, Some(200 * MIB), Some(0))?,
                Decision::Hold(HoldReason::Classification)
            );
        }
        Ok(())
    }

    #[test]
    fn large_sql_migration_stays_source_text() -> io::Result<()> {
        let fixture = Fixture::fresh()?;
        let path = fixture.sized("migration-0042.sql", 120 * MIB)?;
        assert_eq!(classify(&path, &rules())?, Classification::SourceText);
        assert_eq!(
            fixture.route(&path, Some(200 * MIB), Some(0))?,
            Decision::KeepGit
        );
        Ok(())
    }

    #[test]
    fn unknown_archive_is_held_not_enrolled() -> io::Result<()> {
        let fixture = Fixture::fresh()?;
        let path = fixture.sized("bundle.zip", 150 * MIB)?;
        assert_eq!(classify(&path, &rules())?, Classification::Unknown);
        assert_eq!(
            fixture.route(&path, Some(200 * MIB), Some(0))?,
            Decision::Hold(HoldReason::Classification)
        );
        Ok(())
    }

    #[test]
    fn over_100_mib_approved_asset_is_selected_for_lfs() -> io::Result<()> {
        // Narrowly verified selection decision above 100 MiB against the real
        // metadata length; selection is NOT a staging-size exception and does
        // not authorize upload.
        let fixture = Fixture::fresh()?;
        let path = fixture.sized("render-final.png", 101 * MIB)?;
        let evidence = evidence_for(&path, &rules(), context(Some(200 * MIB), Some(0)))?;
        assert_eq!(evidence.bytes, 101 * MIB);
        assert_eq!(evidence.classification, Classification::ApprovedAsset);
        assert_eq!(
            lfs_policy::evaluate(Some(policy()), evidence),
            Decision::EnrollLfs
        );
        Ok(())
    }

    #[test]
    fn sticky_enrollment_survives_shrinkage_on_real_file() -> io::Result<()> {
        let fixture = Fixture::fresh()?;
        let path = fixture.sized("render-final.png", 101 * MIB)?;
        let mut ctx = context(Some(200 * MIB), Some(0));
        ctx.enrolled = true;
        ctx.filter = Filter::Lfs;
        let evidence = evidence_for(&path, &rules(), ctx)?;
        assert_eq!(
            lfs_policy::evaluate(Some(policy()), evidence),
            Decision::RetainLfs
        );
        // Shrink the real file to one byte: representation must be retained.
        File::create(&path)?.set_len(1)?;
        let shrunken = evidence_for(&path, &rules(), ctx)?;
        assert_eq!(shrunken.bytes, 1);
        assert_eq!(
            lfs_policy::evaluate(Some(policy()), shrunken),
            Decision::RetainLfs
        );
        Ok(())
    }

    #[test]
    fn per_repo_override_moves_threshold_for_real_file() -> io::Result<()> {
        let fixture = Fixture::fresh()?;
        let path = fixture.sized("clip-07.mp4", 60 * MIB)?;
        let evidence = evidence_for(&path, &rules(), context(Some(200 * MIB), Some(0)))?;
        assert_eq!(
            lfs_policy::evaluate(Some(policy()), evidence),
            Decision::KeepGit
        );
        let tuned = policy().with_override(PolicyOverride {
            individual_trigger_bytes: Some(50 * MIB),
            ..PolicyOverride::default()
        });
        assert!(tuned.is_valid());
        assert_eq!(
            lfs_policy::evaluate(Some(tuned), evidence),
            Decision::EnrollLfs
        );
        Ok(())
    }
}
