use super::*;
use git2::{IndexEntry, IndexTime};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::process::Command;

const DATA: &[u8] = b"verified original asset";
const REPO_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new(mode: u32) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path().join("repo")).unwrap();
        repo.config()
            .unwrap()
            .set_str("dracon.storageRepoId", REPO_ID)
            .unwrap();
        std::fs::create_dir(repo.workdir().unwrap().join("assets")).unwrap();
        let pointer = Pointer::new(payload()).unwrap().encode();
        std::fs::write(repo.workdir().unwrap().join("assets/private.bin"), &pointer).unwrap();
        let mut index = repo.index().unwrap();
        index
            .add(&IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode,
                uid: 0,
                gid: 0,
                file_size: pointer.len() as u32,
                id: repo.blob(&pointer).unwrap(),
                flags: 0,
                flags_extended: 0,
                path: b"assets/private.bin".to_vec(),
            })
            .unwrap();
        index.write().unwrap();
        std::fs::write(temp.path().join("verified.source"), DATA).unwrap();
        std::fs::set_permissions(
            temp.path().join("verified.source"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        Self { temp }
    }
    fn root(&self) -> &Path {
        self.temp.path()
    }
    fn repo(&self) -> git2::Repository {
        git2::Repository::open(self.root().join("repo")).unwrap()
    }
    fn working(&self) -> PathBuf {
        self.root().join("repo/assets/private.bin")
    }
    fn asset(&self) -> RestoredAsset {
        asset(self.root())
    }
    fn store(&self, limits: Limits) -> HydrationStore {
        HydrationStore::open(&self.root().join("hydration"), REPO_ID, limits).unwrap()
    }
}
fn payload() -> Fingerprint {
    Fingerprint::new("b".repeat(64), 1234).unwrap()
}
fn asset(root: &Path) -> RestoredAsset {
    RestoredAsset {
        path: root.join("verified.source"),
        source: Fingerprint::new(format!("{:x}", Sha256::digest(DATA)), DATA.len() as u64).unwrap(),
        repo_id: REPO_ID.into(),
        path_hex: journal::encode_relative_path(b"assets/private.bin").unwrap(),
        payload: payload(),
    }
}

#[test]
fn hydration_preserves_index_backup_and_cache_and_restores_executable_mode() {
    for mode in [0o100644, 0o100755] {
        let f = Fixture::new(mode);
        let repo = f.repo();
        let index = std::fs::read(repo.path().join("index")).unwrap();
        let store = f.store(Limits::default());
        let result = store.hydrate(&repo, &f.asset()).unwrap();
        assert_eq!(result.path(), f.working());
        assert_eq!(std::fs::read(result.path()).unwrap(), DATA);
        assert_eq!(
            std::fs::read(result.backup().unwrap()).unwrap(),
            Pointer::new(payload()).unwrap().encode()
        );
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
        assert_eq!(
            std::fs::metadata(result.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            if mode == 0o100755 { 0o700 } else { 0o600 }
        );
        let again = store.hydrate(&repo, &f.asset()).unwrap();
        assert_eq!(again.path(), result.path());
        assert!(!repo.path().join("index.lock").exists());
        std::fs::write(result.path(), b"operator edit").unwrap();
        assert_eq!(std::fs::read(f.asset().path()).unwrap(), DATA);
        assert!(store.hydrate(&repo, &f.asset()).is_err());
        assert_eq!(std::fs::read(result.path()).unwrap(), b"operator edit");
    }
}

#[test]
fn missing_working_file_is_published_without_a_backup_or_index_changes() {
    let f = Fixture::new(0o100644);
    let repo = f.repo();
    std::fs::remove_file(f.working()).unwrap();
    let index = std::fs::read(repo.path().join("index")).unwrap();
    let result = f
        .store(Limits::default())
        .hydrate(&repo, &f.asset())
        .unwrap();
    assert!(result.backup().is_none());
    assert_eq!(std::fs::read(result.path()).unwrap(), DATA);
    assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
}

#[test]
fn local_edits_corrupt_cache_wrong_references_and_capacity_never_replace_working_bytes() {
    for failure in [
        "edits",
        "cache",
        "reference",
        "bytes",
        "file-link",
        "parent-link",
        "hard-link",
    ] {
        let f = Fixture::new(0o100644);
        let repo = f.repo();
        let mut limits = Limits::default();
        match failure {
            "edits" => std::fs::write(f.working(), b"operator source edit").unwrap(),
            "cache" => std::fs::write(f.asset().path(), b"corrupt cached output").unwrap(),
            "reference" => {
                let mut index = repo.index().unwrap();
                let mut entry = index.get_path(Path::new("assets/private.bin"), 0).unwrap();
                entry.id = repo.blob(b"ordinary staged raw asset").unwrap();
                index.add(&entry).unwrap();
                index.write().unwrap();
            }
            "bytes" => {
                limits.max_snapshot_bytes = DATA.len() as u64;
                limits.max_retained_snapshot_bytes = DATA.len() as u64;
            }
            "file-link" => {
                std::fs::remove_file(f.working()).unwrap();
                std::fs::write(f.root().join("outside"), b"outside unchanged").unwrap();
                std::os::unix::fs::symlink(f.root().join("outside"), f.working()).unwrap();
            }
            "parent-link" => {
                std::fs::rename(
                    f.root().join("repo/assets"),
                    f.root().join("outside-assets"),
                )
                .unwrap();
                std::os::unix::fs::symlink(
                    f.root().join("outside-assets"),
                    f.root().join("repo/assets"),
                )
                .unwrap();
            }
            "hard-link" => {
                std::fs::hard_link(f.working(), f.root().join("linked-pointer")).unwrap()
            }
            _ => unreachable!(),
        }
        let before = std::fs::read(f.working()).unwrap();
        let index = std::fs::read(repo.path().join("index")).unwrap();
        assert!(
            f.store(limits).hydrate(&repo, &f.asset()).is_err(),
            "{failure}"
        );
        assert_eq!(std::fs::read(f.working()).unwrap(), before, "{failure}");
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
        assert!(!repo.path().join("index.lock").exists());
    }
}

#[test]
fn crash_recovery_resumes_capture_and_publication_without_original_cache_changes() {
    for phase in [
        "after-intent",
        "after-original-capture",
        "before-working-publication",
        "after-working-publication",
    ] {
        let f = Fixture::new(0o100644);
        let repo = f.repo();
        let index = std::fs::read(repo.path().join("index")).unwrap();
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "storage_core::hydration::tests::crash_helper",
                "--ignored",
                "--exact",
            ])
            .env("DRACON_HYDRATION_TEST_ROOT", f.root())
            .env("DRACON_HYDRATION_CRASH_POINT", phase)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(73),
            "{phase}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let receipt = f
            .store(Limits::default())
            .hydrate(&repo, &f.asset())
            .unwrap();
        assert_eq!(std::fs::read(receipt.path()).unwrap(), DATA);
        assert_eq!(
            std::fs::read(receipt.backup().unwrap()).unwrap(),
            Pointer::new(payload()).unwrap().encode()
        );
        assert_eq!(std::fs::read(f.asset().path()).unwrap(), DATA);
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
        assert!(!repo.path().join("index.lock").exists());
    }
}

#[test]
#[ignore = "subprocess helper invoked by hydration crash recovery"]
fn crash_helper() {
    let root = PathBuf::from(std::env::var_os("DRACON_HYDRATION_TEST_ROOT").unwrap());
    let repo = git2::Repository::open(root.join("repo")).unwrap();
    let store = HydrationStore::open(&root.join("hydration"), REPO_ID, Limits::default()).unwrap();
    let result = store.hydrate(&repo, &asset(&root));
    if std::env::var_os("DRACON_HYDRATION_TEST_RACE").is_some() {
        assert!(result.is_err());
    } else {
        result.unwrap();
    }
}

pub(super) fn race_at(phase: &str, parent: &File, name: &OsStr) {
    let Ok(race) = std::env::var("DRACON_HYDRATION_TEST_RACE") else {
        return;
    };
    if race == phase {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).mode(0o600);
        if phase == "before-original-capture" {
            options.truncate(true);
        } else {
            options.create_new(true);
        }
        let mut file = options.open(fd_path(parent).join(name)).unwrap();
        file.write_all(b"concurrent operator edit").unwrap();
        file.sync_all().unwrap();
    } else if race == "parent" && phase == "before-original-capture" {
        let old_parent = std::fs::read_link(fd_path(parent)).unwrap();
        let repo = old_parent.parent().unwrap();
        let outside = repo.parent().unwrap().join("outside-assets");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join(name), b"outside sentinel").unwrap();
        std::fs::rename(&old_parent, repo.join("assets.moved")).unwrap();
        std::os::unix::fs::symlink(&outside, &old_parent).unwrap();
    }
}

#[test]
fn concurrent_edits_creation_and_parent_replacement_preserve_all_operator_bytes() {
    for race in [
        "before-original-capture",
        "before-working-publication",
        "parent",
    ] {
        let f = Fixture::new(0o100644);
        let repo = f.repo();
        let index = std::fs::read(repo.path().join("index")).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "storage_core::hydration::tests::crash_helper",
                "--ignored",
                "--exact",
            ])
            .env("DRACON_HYDRATION_TEST_ROOT", f.root())
            .env("DRACON_HYDRATION_TEST_RACE", race)
            .env_remove("DRACON_HYDRATION_CRASH_POINT")
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{race}: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        let expected: &[u8] = if race == "parent" {
            b"outside sentinel"
        } else {
            b"concurrent operator edit"
        };
        assert_eq!(std::fs::read(f.working()).unwrap(), expected, "{race}");
        assert_eq!(std::fs::read(f.asset().path()).unwrap(), DATA);
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
        assert!(!repo.path().join("index.lock").exists());
        if race == "parent" {
            assert_eq!(
                std::fs::read(f.root().join("repo/assets.moved/private.bin")).unwrap(),
                Pointer::new(payload()).unwrap().encode()
            );
        }
        let transactions = f.root().join("hydration").join(REPO_ID);
        let transaction = std::fs::read_dir(transactions)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.path().is_dir())
            .unwrap();
        assert_eq!(
            std::fs::read(transaction.path().join("publish.source")).unwrap(),
            DATA
        );
        if race == "before-working-publication" {
            assert_eq!(
                std::fs::read(transaction.path().join("original")).unwrap(),
                Pointer::new(payload()).unwrap().encode()
            );
        }
    }
}
