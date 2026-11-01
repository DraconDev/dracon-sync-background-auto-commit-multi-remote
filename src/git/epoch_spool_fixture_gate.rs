//! Compiled only into the unit-test harness. A scoped permit admits the actual
//! public/actor cleanup route for one owned TempDir, never a live/CLI bypass.
//! It changes only the outer qualification interlock, not any proof predicate.
use anyhow::{ensure, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

type Identity = (u64, u64);
fn identity(path: &Path) -> Result<Identity> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir(),
        "fixture repository must be a real directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0,
            "fixture directory must be privately owned"
        );
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!("qualified fixture identity requires Unix")
    }
}
fn permits() -> &'static Mutex<HashMap<Identity, usize>> {
    static PERMITS: OnceLock<Mutex<HashMap<Identity, usize>>> = OnceLock::new();
    PERMITS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) struct ReclaimFixturePermit<'a> {
    identity: Identity,
    _root: &'a tempfile::TempDir,
}
pub(crate) fn allow_reclaim_in_fixture(
    root: &tempfile::TempDir,
) -> Result<ReclaimFixturePermit<'_>> {
    let identity = identity(root.path())?;
    let mut permits = permits()
        .lock()
        .map_err(|_| anyhow::anyhow!("fixture permit registry poisoned"))?;
    let count = permits.entry(identity).or_default();
    *count = count
        .checked_add(1)
        .context("fixture permit count overflow")?;
    Ok(ReclaimFixturePermit {
        identity,
        _root: root,
    })
}
pub(super) fn permitted(repo: &Path) -> bool {
    let Ok(identity) = identity(repo) else {
        return false;
    };
    permits()
        .lock()
        .ok()
        .is_some_and(|permits| permits.get(&identity).copied().unwrap_or(0) > 0)
}
#[test]
fn fixture_permission_is_identity_scoped_and_revoked_on_drop() {
    let one = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    assert!(!permitted(one.path()));
    {
        let permit = allow_reclaim_in_fixture(&one).unwrap();
        assert!(permitted(one.path()));
        assert!(!permitted(other.path()));
        let nested = allow_reclaim_in_fixture(&one).unwrap();
        drop(nested);
        assert!(permitted(one.path()));
        drop(permit);
    }
    assert!(!permitted(one.path()));
}

impl Drop for ReclaimFixturePermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut permits) = permits().lock() {
            if let Some(count) = permits.get_mut(&self.identity) {
                *count -= 1;
                if *count == 0 {
                    permits.remove(&self.identity);
                }
            }
        }
    }
}
