//! Policy-authorized stale conflict-slot reconciliation. No working-file writes.
//! Keep the exact original index AND independent cached payloads before clearing
//! any slot; derive the returned epoch from the private view, never a later read.
use super::*;
#[cfg(test)]
#[path = "unmerged_epoch_tests.rs"]
mod tests;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
};
use tokio::io::AsyncReadExt;

type Stamp = (u64, u64, u64, u32, u32, u32, i64, i64, i64, i64, u64);
fn stamp(meta: &std::fs::Metadata) -> Stamp {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mode(),
        meta.uid(),
        meta.gid(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
        meta.nlink(),
    )
}
struct Source {
    parent: File,
    leaf: CString,
    file: File,
    before: Stamp,
    parent_before: Stamp,
}
impl Source {
    fn open(repo: &Path, path: &Path) -> Result<Self> {
        ensure!(crate::git::is_safe_git_path(path), "unsafe unmerged path");
        let mut parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(repo)?;
        let parts: Vec<_> = path.components().collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            let std::path::Component::Normal(name) = part else {
                anyhow::bail!("non-normal unmerged path")
            };
            let name = CString::new(name.as_bytes())?;
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            ensure!(
                fd >= 0,
                "cannot open unmerged ancestor: {}",
                std::io::Error::last_os_error()
            );
            parent = unsafe { File::from_raw_fd(fd) };
        }
        let Some(std::path::Component::Normal(name)) = parts.last() else {
            anyhow::bail!("missing unmerged basename")
        };
        let leaf = CString::new(name.as_bytes())?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        ensure!(
            fd >= 0,
            "cannot open unmerged source: {}",
            std::io::Error::last_os_error()
        );
        let file = unsafe { File::from_raw_fd(fd) };
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.nlink() == 1,
            "unmerged source must be a single-link regular file"
        );
        Ok(Self {
            before: stamp(&meta),
            parent_before: stamp(&parent.metadata()?),
            parent,
            leaf,
            file,
        })
    }
    fn unchanged(&self, repo: &Path, path: &Path) -> Result<bool> {
        let named = Self::open(repo, path)?;
        Ok(stamp(&self.file.metadata()?) == self.before
            && named.before == self.before
            && stamp(&self.parent.metadata()?) == self.parent_before
            && named.parent_before == self.parent_before
            && self.leaf == named.leaf)
    }
}
async fn index_bytes(path: &Path) -> Result<(Vec<u8>, Stamp)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.nlink() == 1
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.len() <= 64 * 1024 * 1024,
        "unmerged index must be an owned single-link regular bounded file"
    );
    let before = stamp(&meta);
    let mut file = tokio::fs::File::from_std(file);
    let mut bytes = Vec::new();
    (&mut file)
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(
        bytes.len() as u64 == meta.len() && stamp(&file.metadata().await?) == before,
        "unmerged index changed during capture"
    );
    Ok((bytes, before))
}
fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
async fn private_read(repo: &Path, index: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(repo)
        .env("GIT_INDEX_FILE", index)
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .args(args);
    let out = crate::git::run_command_captured_output(
        command.into_tokio(),
        3600,
        "unmerged private observation",
        None,
    )
    .await?;
    ensure!(out.status.success(), "unmerged private read refused");
    Ok(out.stdout)
}
type Slot = (u32, String, u8); // native mode, object ID, stage (0..3)
type Slots = BTreeMap<Vec<u8>, Vec<Slot>>;
fn slots(raw: &[u8], deadline: Instant) -> Result<Slots> {
    let mut paths = Slots::new();
    ensure!(
        raw.is_empty() || raw.ends_with(&[0]),
        "unterminated unmerged observation"
    );
    for record in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        ensure!(Instant::now() < deadline, "unmerged deadline expired");
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .context("invalid unmerged path header")?;
        let fields: Vec<_> = std::str::from_utf8(&record[..tab])?.split(' ').collect();
        ensure!(
            fields.len() == 3
                && crate::git::is_valid_object_id(fields[1])
                && !fields[1].bytes().all(|byte| byte == b'0'),
            "invalid unmerged identity"
        );
        let mode = u32::from_str_radix(fields[0], 8)?;
        let stage: u8 = fields[2].parse()?;
        ensure!(
            matches!(mode, 0o100644 | 0o100755 | 0o120000 | 0o160000) && stage <= 3,
            "unsupported conflict mode/stage"
        );
        let path = record[tab + 1..].to_vec();
        ensure!(
            crate::git::is_safe_git_path(&path_from_git_bytes(&path)?),
            "unsafe conflict path"
        );
        let entries = paths.entry(path).or_default();
        ensure!(
            !entries.iter().any(|(_, _, s)| *s == stage),
            "duplicate conflict stage"
        );
        entries.push((mode, fields[1].to_owned(), stage));
    }
    Ok(paths)
}

/// Keep identity, raw path, stage and flags from ONE native observation.
fn slot_observation(mut raw: &[u8], deadline: Instant) -> Result<Slots> {
    let mut records = Vec::new();
    while !raw.is_empty() {
        ensure!(
            Instant::now() < deadline,
            "unmerged observation deadline expired"
        );
        let nul = raw
            .iter()
            .position(|byte| *byte == 0)
            .context("missing cached path terminator")?;
        let record = &raw[..=nul];
        let parsed = slots(record, deadline)?;
        ensure!(parsed.len() == 1, "ambiguous cached identity");
        let stage = parsed.values().next().context("missing cached stage")?[0].2;
        records.extend_from_slice(record);
        raw = &raw[nul + 1..];
        for prefix in [
            b"  ctime: ".as_slice(),
            b"  mtime: ",
            b"  dev: ",
            b"  uid: ",
            b"  size: ",
        ] {
            let end = raw
                .iter()
                .position(|byte| *byte == b'\n')
                .context("truncated cached debug observation")?;
            let line = &raw[..end];
            ensure!(
                line.starts_with(prefix),
                "unsupported cached debug observation"
            );
            if prefix == b"  size: " {
                let (_, flags) = std::str::from_utf8(line)?
                    .rsplit_once("\tflags: ")
                    .context("missing cached flags")?;
                ensure!(
                    u32::from_str_radix(flags, 16)? == u32::from(stage) << 12,
                    "unsupported cached flags; actual index preserved"
                );
            }
            raw = &raw[end + 1..];
        }
    }
    slots(&records, deadline)
}

/// Recovery is a bounded delta spool, not a duplicate of the whole repository.
const RETAINED_BYTES_LIMIT: u64 = 512 * 1024 * 1024;
fn require_free_space(parent: &Path, reserve: u64) -> Result<()> {
    let path = CString::new(parent.as_os_str().as_bytes())?;
    let mut space = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    ensure!(
        unsafe { libc::statvfs(path.as_ptr(), space.as_mut_ptr()) } == 0,
        "cannot inspect recovery free space: {}",
        std::io::Error::last_os_error()
    );
    let space = unsafe { space.assume_init() };
    let available = u128::from(space.f_bavail) * u128::from(space.f_frsize);
    ensure!(
        available >= u128::from(reserve),
        "insufficient free space for bounded unmerged recovery; actual index preserved"
    );
    Ok(())
}
async fn retention_input(
    repo: &Path,
    objects: &BTreeSet<String>,
    budget: u64,
) -> Result<(String, u64)> {
    let input = objects
        .iter()
        .map(|oid| format!("{oid}\n"))
        .collect::<String>();
    ensure!(
        input.len() <= 16 * 1024 * 1024,
        "unmerged retention input exceeds budget"
    );
    let metadata = git(
        repo,
        &[
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ],
        Some(input.as_bytes()),
    )
    .await?;
    let mut remaining = objects.clone();
    let mut bytes = 0u64;
    for line in std::str::from_utf8(&metadata.stdout)?.lines() {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        ensure!(
            fields.len() == 3 && fields[1] == "blob" && remaining.remove(fields[0]),
            "uncertain retained-object metadata"
        );
        bytes = bytes
            .checked_add(fields[2].parse::<u64>()?)
            .context("recovery size overflow")?;
        ensure!(
            bytes <= budget,
            "unmerged recovery exceeds retained-payload budget; actual index preserved"
        );
    }
    ensure!(remaining.is_empty(), "incomplete retained-object metadata");
    Ok((input, bytes))
}

pub(super) async fn reconcile(
    repo: &Path,
    enabled: bool,
    deadline: Instant,
) -> Result<Option<(Epoch, usize)>> {
    tokio::time::timeout_at(deadline,async {
        // Never treat an active user merge/rebase/cherry-pick as stale slots.
        if check_conflict_state(repo).is_some() { return Ok(None) }
        let unmerged=git(repo,&["ls-files","--unmerged","-z"],None).await?;
        if unmerged.stdout.is_empty() || !enabled { return Ok(None) }
        let conflicts=slots(&unmerged.stdout,deadline)?;
        let actual=git(repo,&["rev-parse","--path-format=absolute","--git-path","index"],None).await?;
        let actual=PathBuf::from(std::str::from_utf8(&actual.stdout)?.trim_end_matches('\n'));
        let mut lock=acquire_epoch_lock(actual.with_extension("lock"))?;
        let reference=super::reference(repo).await?;
        let head=git(repo,&["rev-parse","--verify","HEAD"],None).await?;
        let head=std::str::from_utf8(&head.stdout)?.trim().to_owned();
        ensure!(crate::git::is_valid_object_id(&head),"invalid unmerged parent");
        let (original,original_stamp)=index_bytes(&actual).await?;
        let index_parent=actual.parent().context("index parent missing")?;
        let cached=private_read(repo,&actual,&["ls-files","--stage","--debug","-z"]).await?;
        let all=slot_observation(&cached,deadline)?;
        let observed=index_bytes(&actual).await?;
        ensure!(observed.0==original && observed.1==original_stamp,EpochConflict::IndexChanged);
        ensure!(all.iter().filter(|(_,entries)|entries.iter().any(|(_,_,stage)|*stage!=0)).map(|(p,e)|(p.clone(),e.iter().filter(|(_,_,stage)|*stage!=0).cloned().collect::<Vec<_>>())).collect::<BTreeMap<_,_>>()==conflicts,
            "conflict slots changed before lock; retained original index");
        let tree=git(repo,&["ls-tree","-r","-z","--full-tree",&head],None).await?;
        let base=crate::git::staged_intent::parse_entries(&tree.stdout,false,deadline)?;
        let mut objects=BTreeSet::new();
        for(path,entries)in &all{for(mode,oid,stage)in entries{
            // Always retain conflict payloads, and independent partial staging.
            // Unchanged stage-0 baseline content remains rooted by captured HEAD.
            if *mode!=0o160000 && (*stage!=0 || base.get(path).is_none_or(|entry|entry.oid!=*oid)) {
                objects.insert(oid.clone());
            }
        }}
        let (input,payload_bytes)=retention_input(repo,&objects,RETAINED_BYTES_LIMIT).await?;
        let mut sources=Vec::new();
        for path in conflicts.keys(){
            let Some(entry)=base.get(path) else { continue };
            if !matches!(entry.mode,0o100644|0o100755){continue}
            let path=path_from_git_bytes(path)?;
            let source=match Source::open(repo,&path){Ok(s)=>s,Err(_)=>continue};
            let executable=source.file.metadata()?.mode() & 0o111 != 0;
            if executable!=(entry.mode==0o100755){continue}
            let mut command=crate::policy::tokio_git_command();
            for(key,_)in std::env::vars_os(){if key.to_string_lossy().starts_with("GIT_"){command.env_remove(key);}}
            command.current_dir(repo).env("GIT_NO_LAZY_FETCH","1").env("GIT_OPTIONAL_LOCKS","0")
                .args(["--no-optional-locks","-c","core.fsmonitor=false","hash-object","--no-filters","--stdin"]);
            let out=crate::git::run_command_captured_file(command.into_tokio(),3600,"unmerged held source hash",source.file.try_clone()?).await?;
            ensure!(out.status.success(),"unmerged source hash refused");
            if std::str::from_utf8(&out.stdout)?.trim()!=entry.oid || !source.unchanged(repo,&path)?{continue}
            sources.push((path,source,entry.mode,entry.oid.clone()));
        }
        if sources.len()!=conflicts.len(){
            eprintln!("⚠️ {} unverified conflict content; actual index/worktree preserved, no recovery spool created",repo.display());
            return Ok(None)
        }
        // Only a verified, installable selection can allocate a retained spool.
        // Stable unknown/flagged retries must not duplicate payloads forever.
        require_free_space(index_parent,payload_bytes + original.len() as u64 * 3 + 64 * 1024 * 1024)?;
        let directory=tempfile::Builder::new().prefix("dracon-unmerged-recovery-")
            .permissions(std::fs::Permissions::from_mode(0o700)).tempdir_in(index_parent)?.keep();
        let candidate=directory.join("index");
        private_file(&directory.join("original-index"),&original)?; private_file(&candidate,&original)?;
        let prefix=directory.join("retained");
        git(repo,&["pack-objects","--revs","--window=0","--depth=0",prefix.to_str().context("unsupported unmerged retention path")?],Some(input.as_bytes())).await?;
        for entry in std::fs::read_dir(&directory)?{let path=entry?.path();if path.extension().is_some_and(|e|e=="pack"||e=="idx"){File::open(path)?.sync_all()?;}}
        for(path,_,mode,oid)in &sources {
            // Exact path bytes; no pathspec expansion, filtering or worktree write.
            let mut update=format!("0 {}\t", "0".repeat(oid.len())).into_bytes();
            update.extend_from_slice(path.as_os_str().as_bytes());update.push(0);
            update.extend_from_slice(format!("{mode:o} {oid}\t").as_bytes());
            update.extend_from_slice(path.as_os_str().as_bytes());update.push(0);
            let mut command=crate::policy::tokio_git_command();
            for(key,_)in std::env::vars_os(){if key.to_string_lossy().starts_with("GIT_"){command.env_remove(key);}}
            command.current_dir(repo).env("GIT_INDEX_FILE",&candidate).env("GIT_NO_LAZY_FETCH","1")
                .args(["update-index","-z","--index-info"]);
            let out=crate::git::run_command_captured_output(command.into_tokio(),3600,"private conflict selection",Some(&update)).await?;
            ensure!(out.status.success(),"private conflict selection failed");
        }
        // Remaining uncertain slots deliberately refuse the whole install.
        let snapshot=StagedIntentSnapshot::capture_index(repo,Some(&candidate),deadline).await?;
        ensure!(snapshot.head==CapturedHead::Commit(head.clone()),"parent changed during conflict selection");
        private_file(&directory.join("recovery.json"),&serde_json::to_vec_pretty(&serde_json::json!({
            "source":env!("DRACON_EPOCH_SOURCE_ID"),"head":head,"reference":reference,
            "resolved":sources.iter().map(|(p,_,_,_)|p.as_os_str().as_bytes()).collect::<Vec<_>>(),
            "action":"policy-enabled HEAD-equal stale conflict selection; original-index and cached pack retained"}))?)?;
        File::open(&directory)?.sync_all()?;
        ensure!(lock.is_owned(),EpochConflict::LockReplaced);
        let now=index_bytes(&actual).await?;
        ensure!(now.0==original && now.1==original_stamp,EpochConflict::IndexChanged);
        ensure!(reference==super::reference(repo).await?,EpochConflict::ReferenceChanged);
        let current_head=git(repo,&["rev-parse","--verify","HEAD"],None).await?;
        ensure!(std::str::from_utf8(&current_head.stdout)?.trim()==head,EpochConflict::ReferenceChanged);
        for(path,source,_,_)in &sources{ensure!(source.unchanged(repo,path)?,"unmerged working source changed; original index retained");}
        // Child-directory fsync alone does not persist its name in the Git
        // directory. Make recovery reachable BEFORE replacing the actual index.
        File::open(index_parent)?.sync_all()?;
        ensure!(Instant::now()<deadline,"unmerged selection deadline expired");
        lock.install_index(repo,&candidate,&actual,deadline).await?;
        File::open(index_parent)?.sync_all().with_context(||format!("index installed; original recovery retained at {}",directory.display()))?;
        eprintln!("🛟 {} reconciled {} HEAD-equal stale conflict paths; exact original index/payloads retained at {}",repo.display(),sources.len(),directory.display());
        Ok(Some((Epoch{snapshot,reference,deadline,retained:Some(directory.clone()),spool_lease:crate::git::SpoolLease::new(Some(directory)),deletions:std::collections::BTreeMap::new(),ignore_patterns:Vec::new()},sources.len())))
    }).await.context("unmerged reconciliation deadline expired; actual data and private recovery retained")?
}
