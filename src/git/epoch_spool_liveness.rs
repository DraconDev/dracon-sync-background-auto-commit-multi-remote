//! Live-writer observation for legacy spools. A legacy spool has no kernel
//! lease, so the reclaimer must establish inactivity with an observable
//! cross-process fence before it may touch the source at all.
use anyhow::Result;
use std::path::Path;

/// Every process holding a descriptor, working directory, root or Git
/// environment that points inside `spool`. An empty result is the observation
/// that no live writer referenced the source at this moment.
pub(super) fn live_handles(spool: &Path) -> Result<Vec<String>> {
    let mut found = Vec::new();
    // Without /proc there is no observation, so no inactivity can be proven.
    let entries = std::fs::read_dir("/proc")?;
    let needle = spool.as_os_str().as_encoded_bytes();
    let ours = std::process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let pid: u32 = match name.parse() {
            Ok(pid) => pid,
            Err(_) => continue,
        };
        if pid == ours {
            continue;
        }
        let process = entry.path();
        for link in ["cwd", "root"] {
            if let Ok(target) = std::fs::read_link(process.join(link)) {
                if target.as_os_str().as_encoded_bytes().starts_with(needle) {
                    found.push(format!("{name}/{link}"));
                }
            }
        }
        if let Ok(descriptors) = std::fs::read_dir(process.join("fd")) {
            for descriptor in descriptors.flatten() {
                if let Ok(target) = std::fs::read_link(descriptor.path()) {
                    if target.as_os_str().as_encoded_bytes().starts_with(needle) {
                        found.push(format!(
                            "{name}/fd/{}",
                            descriptor.file_name().to_string_lossy()
                        ));
                    }
                }
            }
        }
        // A Git producer points GIT_INDEX_FILE/GIT_OBJECT_DIRECTORY at the spool
        // while its descriptors are momentarily closed.
        if let Ok(bytes) = std::fs::read(process.join("environ")) {
            for variable in bytes.split(|byte| *byte == 0) {
                if variable
                    .windows(needle.len())
                    .any(|window| window == needle)
                {
                    found.push(format!("{name}/environ"));
                    break;
                }
            }
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}
