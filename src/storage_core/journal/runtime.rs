//! Prevent private runtime versions from entering a containing Git repository.

use super::*;
use std::process::Stdio;

const IGNORE: &[u8] = b"# Dracon Sync private runtime state; never commit.\n*\n";

pub(super) fn protect(directory: &Path) -> Result<()> {
    // Never write a blanket ignore into a project's own root.
    if std::fs::symlink_metadata(directory.join(".git")).is_ok() {
        bail!("a Git repository root cannot be private runtime storage");
    }
    refuse_tracked(directory)?;
    let path = directory.join(".gitignore");
    if exists_without_symlink(&path)? {
        return verify_ignore(&path);
    }
    let _bootstrap = try_lock(&directory.join(".runtime-ignore.lock"))?;
    if exists_without_symlink(&path)? {
        return verify_ignore(&path);
    }
    // The bootstrap spool contains only a public ignore rule, never a source,
    // hash, key or job record. Payload creation starts after its durable rename.
    let temporary = directory.join(".runtime-ignore.tmp");
    let mut file = open_private(&temporary, true, true)?;
    file.set_len(0)?;
    file.write_all(IGNORE)?;
    file.sync_all()?;
    std::fs::rename(temporary, &path)?;
    File::open(directory)?.sync_all()?;
    verify_ignore(&path)
}

fn verify_ignore(path: &Path) -> Result<()> {
    let mut file = open_private(path, false, false)?;
    let mut bytes = Vec::new();
    file.take(IGNORE.len() as u64 + 1).read_to_end(&mut bytes)?;
    if bytes != IGNORE {
        bail!("private runtime ignore protection was changed; payload writes refused");
    }
    Ok(())
}

fn refuse_tracked(directory: &Path) -> Result<()> {
    let Some(repo) = directory
        .ancestors()
        .skip(1)
        .find(|ancestor| std::fs::symlink_metadata(ancestor.join(".git")).is_ok())
    else {
        return Ok(());
    };
    let relative = directory.strip_prefix(repo)?;
    let mut command = std::process::Command::new("git");
    command
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "ls-files",
            "--cached",
            "-z",
            "--",
        ])
        .arg(relative)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_CEILING_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(name);
    }
    let mut child = command
        .spawn()
        .context("cannot verify private runtime Git isolation")?;
    let mut byte = [0u8; 1];
    let count = child
        .stdout
        .take()
        .context("missing Git isolation output")?
        .read(&mut byte);
    if !matches!(count, Ok(0)) {
        let _ = child.kill();
        let _ = child.wait();
        bail!("tracked or unreadable runtime paths cannot receive private payloads");
    }
    if !child.wait()?.success() {
        bail!("cannot verify private runtime Git isolation");
    }
    Ok(())
}
