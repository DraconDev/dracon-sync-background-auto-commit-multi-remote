//! Create-only, shared physical storage; each record remains independently usable.
//! No CAS pruning, alternate dependencies or main-repository refs are introduced.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

fn directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(metadata.is_dir(), "CAS directory must not be a symlink");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0,
            "CAS directory must be operator-owned and not externally writable"
        );
    }
    Ok(())
}

/// Move only transaction-owned unpublished pack bytes into private work storage,
/// then unpack in isolation. Existing retained recovery artifacts are NEVER moved,
/// replaced or removed. Final record links are verified by the caller before its
/// receipt; a corrupt existing CAS candidate therefore cannot authorize deletion.
pub(super) async fn share(
    work: &Path,
    record: &Path,
    rescue: &Path,
    format: &str,
    hash: &str,
    timeout: u64,
) -> Result<()> {
    let store = record.join("store.git");
    let packdir = store.join("objects/pack");
    let packed = work.join("complete.pack");
    fs::rename(packdir.join(format!("pack-{hash}.pack")), &packed)?;
    fs::rename(
        packdir.join(format!("pack-{hash}.idx")),
        work.join("complete.idx"),
    )?;
    let count = pack_count(&packed)?;
    let unpacked = work.join("unpacked.git");
    init(&unpacked, format, timeout).await?;
    let mut command = git(&unpacked);
    command
        .args(["unpack-objects", "--strict"])
        .stdin(File::open(&packed)?);
    run(command, timeout).await?;
    verify(&unpacked, &record.join("roots"), timeout).await?;

    let inventory = record.join("objects.tsv");
    let mut command = git(&unpacked);
    command
        .args([
            "cat-file",
            "--batch-all-objects",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ])
        .stdout(File::create(&inventory)?);
    run(command, timeout).await?;
    let width = if format == "sha1" { 40 } else { 64 };
    let mut objects = BTreeSet::new();
    for line in BufReader::new(File::open(&inventory)?).lines() {
        let line = line?;
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(
            fields.len() == 3
                && oid(fields[0], width)
                && ["blob", "tree", "commit", "tag"].contains(&fields[1]),
            "invalid unpacked object inventory"
        );
        fields[2].parse::<u64>()?;
        ensure!(
            objects.insert(fields[0].to_owned()),
            "duplicate unpacked object"
        );
    }
    ensure!(
        objects.len() == count,
        "complete pack/unpacked inventory mismatch"
    );
    let cas = rescue.join("cas");
    directory(&cas)?;
    let cas_objects = cas.join("objects");
    directory(&cas_objects)?;
    for object in objects {
        let source = unpacked
            .join("objects")
            .join(&object[..2])
            .join(&object[2..]);
        regular(&source)?;
        let fanout = cas_objects.join(&object[..2]);
        directory(&fanout)?;
        let shared = fanout.join(&object[2..]);
        match fs::hard_link(&source, &shared) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Existing means unverified, NOT correct. Final isolated fsck
                // hashes every reused object. No repair or overwrite is allowed.
                regular(&shared)?;
            }
            Err(error) => return Err(error.into()),
        }
        regular(&shared)?.sync_all()?;
        File::open(&fanout)?.sync_all()?;
        let local = store.join("objects").join(&object[..2]);
        directory(&local)?;
        fs::hard_link(&shared, local.join(&object[2..]))?;
        File::open(&local)?.sync_all()?;
    }
    File::open(&cas_objects)?.sync_all()?;
    File::open(&cas)?.sync_all()?;
    File::open(rescue)?.sync_all()?;
    // Private record-only roots, including dangling objects from every retained
    // pack. Gitlinks are deliberately not external-repository commit roots.
    let mut refs = String::from("# pack-refs with: sorted\n");
    for root in fs::read_to_string(record.join("roots"))?.lines() {
        ensure!(oid(root, width), "invalid recovery root");
        refs.push_str(&format!("{root} refs/dracon-recovery/{root}\n"));
    }
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(store.join("packed-refs"))?;
    target.write_all(refs.as_bytes())?;
    target.sync_all()?;
    File::open(store.join("objects"))?.sync_all()?;
    File::open(&store)?.sync_all()?;
    Ok(())
}

/// Share identical recovery metadata bytes across records. Recovery records
/// must stay independent, but the same staging index is repeated in many
/// spools; content-addressed copies keep the recovery store proportional to
/// distinct content instead of to the number of records. A shared file is
/// immutable: the manifest still binds its hash, and verification still reads
/// the record's own file.
pub(super) fn share_metadata(record: &Path, rescue: &Path) -> Result<()> {
    let metadata = record.join("metadata");
    let cas = rescue.join("cas");
    directory(&cas)?;
    let shared_root = cas.join("meta");
    directory(&shared_root)?;
    let mut entries: Vec<_> = fs::read_dir(&metadata)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        ensure!(
            entry.file_type()?.is_file(),
            "recovery metadata must be a regular file"
        );
        let mut digest = Sha256::new();
        let mut file = regular(&path)?;
        let mut buffer = [0; 65536];
        loop {
            let length = file.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            digest.update(&buffer[..length]);
        }
        let hash = format!("{:x}", digest.finalize());
        let shared = shared_root.join(&hash);
        match fs::hard_link(&path, &shared) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Existing content is reused only when it is byte-identical.
                let mut existing = Sha256::new();
                let mut file = regular(&shared)?;
                loop {
                    let length = file.read(&mut buffer)?;
                    if length == 0 {
                        break;
                    }
                    existing.update(&buffer[..length]);
                }
                ensure!(
                    format!("{:x}", existing.finalize()) == hash,
                    "shared recovery metadata differs from the source bytes"
                );
            }
            Err(error) => return Err(error.into()),
        }
        regular(&shared)?.sync_all()?;
        File::open(&shared_root)?.sync_all()?;
        // Replace the record's copy with a link to the shared immutable bytes.
        let replacement = record.join(format!(".shared-{}", entry.file_name().to_string_lossy()));
        fs::rename(&path, &replacement)?;
        fs::hard_link(&shared, &path)?;
        fs::remove_file(&replacement)?;
    }
    File::open(&shared_root)?.sync_all()?;
    File::open(&cas)?.sync_all()?;
    Ok(())
}
