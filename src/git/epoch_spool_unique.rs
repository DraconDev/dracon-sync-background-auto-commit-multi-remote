//! Compact payload absent from the main ODB at classification time. This is an
//! accounting/export sidecar, NEVER the durability authority: store.git retains
//! the complete independent recovery closure even for a duplicate classification.
use super::*;

pub(super) struct Payload {
    pub count: usize,
    pub uncompressed_bytes: u64,
    pub packed_bytes: u64,
    pub pack_name: Option<PathBuf>,
}

pub(super) async fn prepare(
    repo: &Path,
    work: &Path,
    record: &Path,
    width: usize,
    timeout: u64,
    corrupt_after_write: bool,
) -> Result<Payload> {
    let requested = fs::read_to_string(record.join("source-oids"))?;
    let mut command = git(repo);
    command
        .args(["cat-file", "--batch-check=%(objectname) %(objecttype)"])
        .stdin(File::open(record.join("source-oids"))?);
    let output = String::from_utf8(run(command, timeout).await?)?;
    let rows: Vec<_> = output.lines().collect();
    ensure!(
        rows.len() == requested.lines().count(),
        "incomplete main ODB classification"
    );
    let mut missing = BTreeSet::new();
    for (object, row) in requested.lines().zip(rows) {
        let fields: Vec<_> = row.split_whitespace().collect();
        ensure!(
            oid(object, width) && fields.len() == 2 && fields[0] == object,
            "invalid main classification row"
        );
        if fields[1] == "missing" {
            missing.insert(object.to_owned());
        } else {
            ensure!(
                ["blob", "tree", "commit", "tag"].contains(&fields[1]),
                "unreadable main ODB classification"
            );
        }
    }
    let missing_path = record.join("missing-oids");
    fs::write(
        &missing_path,
        missing
            .iter()
            .map(|oid| oid.clone() + "\n")
            .collect::<String>(),
    )?;
    if missing.is_empty() {
        return Ok(Payload {
            count: 0,
            uncompressed_bytes: 0,
            packed_bytes: 0,
            pack_name: None,
        });
    }
    let store = record.join("store.git");
    let mut command = git(&store);
    command
        .args(["pack-objects", "--no-reuse-object", "--no-reuse-delta"])
        .arg(record.join("rescue"))
        .stdin(File::open(&missing_path)?);
    let hash = String::from_utf8(run(command, timeout).await?)?
        .trim()
        .to_owned();
    ensure!(oid(&hash, width), "invalid unique pack name");
    let name = PathBuf::from(format!("rescue-{hash}.pack"));
    let pack = record.join(&name);
    let isolated = work.join("unique.git");
    let format = if width == 40 { "sha1" } else { "sha256" };
    init(&isolated, format, timeout).await?;
    let mut command = git(&isolated);
    command.arg("index-pack").arg(&pack);
    run(command, timeout).await?;
    #[cfg(test)]
    if corrupt_after_write {
        let mut bytes = fs::read(&pack)?;
        ensure!(bytes.len() > 20, "empty corruption fixture pack");
        bytes[12] ^= 0x55;
        let mut permissions = fs::metadata(&pack)?.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        fs::set_permissions(&pack, permissions)?;
        fs::write(&pack, bytes)?;
    }
    #[cfg(not(test))]
    let _ = corrupt_after_write;
    let listing = work.join("unique-listing");
    let mut command = git(&isolated);
    command
        .args(["verify-pack", "-v"])
        .arg(pack.with_extension("idx"))
        .stdout(File::create(&listing)?);
    run(command, timeout).await?;
    ensure!(
        listed_objects(&listing, width, pack_count(&pack)?)? == missing,
        "unique rescue enumeration differs from requested objects"
    );
    // The isolated ODB has only the new sidecar and its index, no refs, source,
    // inherited/configured alternates, or main repository fallback.
    let target = isolated
        .join("objects/pack")
        .join(format!("pack-{hash}.pack"));
    fs::hard_link(&pack, &target)?;
    fs::hard_link(pack.with_extension("idx"), target.with_extension("idx"))?;
    let mut command = git(&isolated);
    command
        .args(["cat-file", "--batch"])
        .stdin(File::open(&missing_path)?)
        .stdout(Stdio::null());
    run(command, timeout).await?;
    let mut command = git(&isolated);
    command
        .args([
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ])
        .stdin(File::open(&missing_path)?);
    let readback = String::from_utf8(run(command, timeout).await?)?;
    let inventory = fs::read_to_string(record.join("objects.tsv"))?;
    let inventory: BTreeSet<_> = inventory.lines().collect();
    ensure!(
        readback.lines().count() == missing.len(),
        "incomplete isolated unique readback"
    );
    let mut uncompressed_bytes = 0u64;
    for (expected, actual) in missing.iter().zip(readback.lines()) {
        ensure!(
            actual.split_whitespace().next() == Some(expected.as_str())
                && inventory.contains(actual),
            "isolated unique object/content identity mismatch"
        );
        let size: u64 = actual
            .split_whitespace()
            .nth(2)
            .context("missing unique object size")?
            .parse()?;
        uncompressed_bytes = uncompressed_bytes
            .checked_add(size)
            .context("unique byte count overflow")?;
    }
    fs::write(record.join("unique-readback.tsv"), readback)?;
    Ok(Payload {
        count: missing.len(),
        uncompressed_bytes,
        packed_bytes: fs::metadata(&pack)?.len(),
        pack_name: Some(name),
    })
}
