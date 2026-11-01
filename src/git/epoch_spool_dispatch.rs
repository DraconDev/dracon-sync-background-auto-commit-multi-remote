//! Explicit adapter for private-epoch Git plumbing, not generic Command wrapping.
//! Captured endpoints are original-spool files: cancellation cannot clean them
//! beneath a live service. They remain recovery metadata until guarded reclaim.
use anyhow::{ensure, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Output;

fn endpoint(spool: &Path, input: Option<&[u8]>) -> Result<PathBuf> {
    let file = tempfile::Builder::new()
        .prefix("producer-io-")
        .tempfile_in(spool)?;
    let (mut file, path) = file.keep()?;
    if let Some(input) = input {
        file.write_all(input)?;
    }
    Ok(path)
}

fn captured(path: &Path) -> Result<Vec<u8>> {
    const LIMIT: usize = 16 * 1024 * 1024;
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= LIMIT as u64,
        "epoch producer capture limit exceeded"
    );
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= LIMIT,
        "epoch producer capture grew beyond limit"
    );
    Ok(bytes)
}

/// None explicitly means this source has no qualified containment scope. It can
/// still be staged by the historical runner, but can never mint a settled marker.
/// Closed or already-settled sources return an error, never a silent fallback.
pub(crate) async fn run_spool_git(
    repo: &Path,
    spool: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    index: Option<&Path>,
) -> Result<Option<Output>> {
    let Some(scope) = super::ownership::producer_scope(spool)? else {
        return Ok(None);
    };
    let command = crate::policy::tokio_git_command();
    // This factory has only sealed environment overrides: no env_clear, custom
    // pre_exec, resource settings or inherited descriptor protocol to recover.
    let command = command.as_std();
    let mut changes: Vec<_> = std::env::vars_os()
        .filter_map(|(key, _)| {
            key.to_str()
                .filter(|key| key.starts_with("GIT_"))
                .map(|key| (key.to_owned(), None))
        })
        .collect();
    for (key, value) in command.get_envs() {
        changes.push((
            key.to_str()
                .context("non-UTF8 Git environment name")?
                .to_owned(),
            value
                .map(|value| value.to_str().context("non-UTF8 Git environment value"))
                .transpose()?
                .map(str::to_owned),
        ));
    }
    changes.extend([
        ("GIT_NO_LAZY_FETCH".into(), Some("1".into())),
        ("GIT_OPTIONAL_LOCKS".into(), Some("0".into())),
        ("GIT_TERMINAL_PROMPT".into(), Some("0".into())),
    ]);
    if let Some(index) = index {
        ensure!(
            index.is_absolute() && index.parent() == Some(spool),
            "private index outside owned spool"
        );
        changes.push((
            "GIT_INDEX_FILE".into(),
            Some(index.to_str().context("non-UTF8 private index")?.to_owned()),
        ));
    }
    let environment = super::producer::producer_environment(changes)?;
    ensure!(
        !environment.contains_key("DRACON_CANONICAL_WRITER_FD_V1"),
        "epoch service cannot transfer a canonical-writer descriptor"
    );
    let cwd = repo.canonicalize()?;
    let program = Path::new(command.get_program());
    let executable = if program.is_absolute() {
        program.canonicalize()?
    } else if program.components().count() > 1 {
        cwd.join(program).canonicalize()?
    } else {
        std::env::split_paths(std::ffi::OsStr::new(
            environment.get("PATH").context("Git PATH missing")?,
        ))
        .map(|path| {
            if path.is_absolute() {
                path.join(program)
            } else {
                cwd.join(path).join(program)
            }
        })
        .find(|path| path.is_file())
        .context("Git executable unavailable")?
        .canonicalize()?
    };
    let stdin = input
        .map(|bytes| endpoint(spool, Some(bytes)))
        .transpose()?;
    let stdout = endpoint(spool, None)?;
    let stderr = endpoint(spool, None)?;
    let recipe = super::producer::EpochProducerCommand {
        executable,
        cwd,
        argv: args.iter().map(|arg| (*arg).to_owned()).collect(),
        environment,
        stdin,
        stdout: Some(stdout.clone()),
        stderr: Some(stderr.clone()),
    };
    let result = scope.run(recipe)?.await?;
    #[cfg(unix)]
    let status = {
        use std::os::unix::process::ExitStatusExt;
        match result {
            super::producer::ProducerExit::Code(code) => {
                ensure!(
                    (0..=255).contains(&code),
                    "invalid epoch producer exit code"
                );
                std::process::ExitStatus::from_raw(code << 8)
            }
            super::producer::ProducerExit::Signal(signal) => {
                ensure!((1..=127).contains(&signal), "invalid epoch producer signal");
                std::process::ExitStatus::from_raw(signal)
            }
        }
    };
    #[cfg(not(unix))]
    let status = {
        let _ = result;
        anyhow::bail!("epoch service exit status requires Unix");
    };
    Ok(Some(Output {
        status,
        stdout: captured(&stdout)?,
        stderr: captured(&stderr)?,
    }))
}
