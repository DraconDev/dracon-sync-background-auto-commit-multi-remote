//! The single irreversible boundary used by automatic and settled-owner cleanup.
//! Preparation retains the kernel fence even if this request is cancelled. No
//! detached preparation task removes a source. Published records are never reaped.
use super::{ownership::ReclaimOwner, recovery};
use anyhow::{ensure, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) enum Fault {
    None,
    #[cfg(test)]
    CorruptUnique,
    #[cfg(test)]
    AfterReceipt,
    #[cfg(test)]
    ChangeSource,
    #[cfg(test)]
    FailLedger,
    #[cfg(test)]
    AfterIntent,
}
fn protected(spool: &Path) -> bool {
    ["preserve-rejected", "original-index"].iter().any(|name| {
        !matches!(fs::symlink_metadata(spool.join(name)), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    })
}
fn durable_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    File::open(path.parent().context("no journal parent")?)?.sync_all()?;
    Ok(())
}
fn append_intent(rescue: &Path, value: &serde_json::Value) -> Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(rescue.join("reclaim-ledger.jsonl"))?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "reclaim ledger is not a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() }
                && metadata.nlink() == 1
                && metadata.mode() & 0o022 == 0,
            "unsafe reclaim ledger ownership/permissions"
        );
    }
    file.try_lock()
        .context("another transaction is journaling; source retained")?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    File::open(rescue)?.sync_all()?;
    Ok(())
}

pub(super) async fn attempt(
    repo: &Path,
    spool: &Path,
    seconds: u64,
    fault: Fault,
) -> Result<recovery::DurableReceipt> {
    let seconds = seconds.clamp(1, 600);
    tokio::time::timeout(
        std::time::Duration::from_secs(seconds),
        inner(repo, spool, seconds, fault),
    )
    .await
    .context("reclamation transaction deadline; no removal authority")?
}
async fn inner(
    repo: &Path,
    spool: &Path,
    seconds: u64,
    _fault: Fault,
) -> Result<recovery::DurableReceipt> {
    let gitdir = super::absolute_git_dir(repo)
        .await
        .context("cannot establish originating gitdir")?;
    let owner = ReclaimOwner::acquire(&gitdir, spool)?;
    ensure!(
        owner.is_settled(),
        "producer quiescence uncertain: no matching settled marker"
    );
    ensure!(!protected(spool), "preserve-marked source");
    let original_bytes = super::directory_bytes(spool)?;
    #[cfg(test)]
    let (receipt, owner) = if matches!(_fault, Fault::CorruptUnique) {
        recovery::prepare_owned_fault(repo, spool, seconds, owner, recovery::Fault::CorruptUniqueBeforeVerify).await?
    } else {
        recovery::prepare_owned_record(repo, spool, seconds, owner).await?
    };
    #[cfg(not(test))]
    let (receipt, owner) = recovery::prepare_owned_record(repo, spool, seconds, owner).await?;
    #[cfg(test)]
    {
        if matches!(_fault, Fault::AfterReceipt) {
            anyhow::bail!("injected crash boundary after durable receipt");
        }
        if matches!(_fault, Fault::ChangeSource) {
            fs::write(spool.join("notes"), b"changed after receipt")?;
        }
    }
    recovery::verify_receipt(&receipt.record, &receipt.manifest_sha256, seconds).await?;
    let owner = recovery::verify_source_owned(&receipt.record, spool, owner).await?;
    ensure!(
        owner.still_owned() && owner.is_settled() && !protected(spool),
        "source ownership or preservation changed"
    );
    let intent = serde_json::json!({
        "version": 1, "phase": "authorized-not-yet-removed",
        "spool_path_bytes": spool.as_os_str().as_encoded_bytes(),
        "record_path_bytes": receipt.record.as_os_str().as_encoded_bytes(),
        "manifest_sha256": receipt.manifest_sha256,
        "original_logical_bytes": original_bytes,
        "unique_objects": receipt.unique_objects,
        "unique_uncompressed_bytes": receipt.unique_uncompressed_bytes,
        "rescue_pack_bytes": receipt.rescue_pack_bytes,
        "rescue_pack_path_bytes": receipt.rescue_pack.as_ref().map(|p| p.as_os_str().as_encoded_bytes()),
        "verified_unix_seconds": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
    });
    durable_json(&receipt.record.join("reclaim-intent.json"), &intent)?;
    #[cfg(test)]
    if matches!(_fault, Fault::FailLedger) {
        anyhow::bail!("injected ledger durability failure");
    }
    append_intent(&gitdir.join("dracon-rescue"), &intent)?;
    #[cfg(test)]
    if matches!(_fault, Fault::AfterIntent) {
        anyhow::bail!("injected crash boundary after intent, before removal");
    }
    // No await after these final checks: a cancelled request cannot resume later
    // and remove a source. All surviving preparations retain data, never delete.
    ensure!(
        owner.still_owned() && owner.is_settled() && !protected(spool),
        "final ownership/preserve check failed"
    );
    recovery::final_source_check(&receipt.record, spool)?;
    fs::remove_dir_all(spool).context("source removal failed; independent recovery retained")?;
    File::open(&gitdir)?
        .sync_all()
        .context("source removed, directory durability unconfirmed; recovery retained")?;
    let mut removed = intent;
    removed["phase"] = "removed".into();
    durable_json(&receipt.record.join("reclaimed.json"), &removed)
        .context("source removed but completion journal failed; recovery retained")?;
    Ok(receipt)
}
