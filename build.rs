use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn rust_sources(root: &Path, paths: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_sources(&path, paths);
        } else if path.extension().is_some_and(|e| e == "rs") {
            paths.push(path);
        }
    }
}
fn main() {
    let mut files = Vec::new();
    rust_sources(Path::new("src"), &mut files);
    files.extend([PathBuf::from("build.rs"), PathBuf::from("Cargo.toml")]);
    // Only the actual utilities workspace owns the enclosing lock; an
    // unrelated parent Cargo.toml is not a dependency-resolution identity.
    let parent_workspace = fs::read_to_string("../Cargo.toml")
        .ok()
        .is_some_and(|text| text.contains("[workspace]") && text.contains("\"dracon-sync\""));
    let lock = if parent_workspace {
        PathBuf::from("../Cargo.lock")
    } else {
        PathBuf::from("Cargo.lock")
    };
    println!("cargo:rerun-if-changed={}", lock.display());
    let lock_missing = !lock.is_file();
    if !lock_missing {
        files.push(lock);
    }
    files.sort();
    let mut hash = Sha256::new();
    hash.update(b"dracon-staged-epoch-v1\0");
    hash.update(if lock_missing {
        b"lock:absent".as_slice()
    } else {
        b"lock:present".as_slice()
    });
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(&path).expect("source identity input");
        hash.update(path.to_string_lossy().as_bytes());
        hash.update([0]);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    let mut settings: Vec<_> = env::vars()
        .filter(|(k, _)| k == "TARGET" || k.starts_with("CARGO_FEATURE_"))
        .collect();
    settings.sort();
    for (key, value) in settings {
        hash.update(key);
        hash.update([0]);
        hash.update(value);
        hash.update([0]);
    }
    println!(
        "cargo:rustc-env=DRACON_EPOCH_SOURCE_ID={:x}",
        hash.finalize()
    );
}
