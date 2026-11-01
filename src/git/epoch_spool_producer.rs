//! Epoch-only producer containment, not a generic Command adapter or a reaper.
//! Same-UID deliberate migration to another cgroup is outside this trust boundary.
//! No source storage is deleted and no settled marker is written here.
#![allow(dead_code)] // Deliberately disconnected until the ownership integration is reviewed.

use anyhow::{bail, ensure, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const TRAMPOLINE: &str = "--dracon-epoch-producer-exec";
const POLL: Duration = Duration::from_millis(30);

/// Explicit supported recipe: no pre_exec, inherited descriptors, or Command introspection.
/// File endpoints must remain owned by the caller until settlement, even on cancellation.
pub(crate) struct EpochProducerCommand {
    pub executable: PathBuf,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub environment: BTreeMap<String, String>,
    pub stdin: Option<PathBuf>,
    pub stdout: Option<PathBuf>,
    pub stderr: Option<PathBuf>,
}

/// Snapshot inheritance explicitly, then apply declared removals/overrides.
/// Non-UTF8 environments are unsupported rather than silently changed.
pub(crate) fn producer_environment(
    changes: impl IntoIterator<Item = (String, Option<String>)>,
) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for (key, value) in std::env::vars_os() {
        result.insert(
            key.into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF8 environment name"))?,
            value
                .into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF8 environment value"))?,
        );
    }
    apply_changes(&mut result, changes);
    validate_environment(&result)?;
    Ok(result)
}

fn apply_changes(
    env: &mut BTreeMap<String, String>,
    changes: impl IntoIterator<Item = (String, Option<String>)>,
) {
    for (key, value) in changes {
        if let Some(value) = value {
            env.insert(key, value);
        } else {
            env.remove(&key);
        }
    }
}

fn validate_environment(env: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in env {
        ensure!(
            !key.is_empty()
                && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && !key.as_bytes()[0].is_ascii_digit(),
            "unsupported environment name"
        );
        ensure!(!value.contains('\0'), "NUL environment value");
    }
    Ok(())
}

fn absolute_text(path: &Path) -> Result<&str> {
    ensure!(path.is_absolute(), "producer paths must be absolute");
    let text = path.to_str().context("non-UTF8 producer path")?;
    ensure!(
        !text.contains(['\0', '\n', '\r', '%']),
        "unsupported producer path"
    );
    Ok(text)
}

/// Dispatch before runtime_env and before any Tokio threads exist. Exec uses an
/// explicit environment, never process-global mutation. Manager-added GIT_* and
/// other undeclared variables cannot reach the actual producer.
pub(crate) fn epoch_producer_trampoline() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != TRAMPOLINE) {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let count: usize = args
            .get(2)
            .and_then(|s| s.to_str())
            .context("missing environment count")?
            .parse()?;
        ensure!(
            count <= args.len().saturating_sub(4),
            "invalid environment count"
        );
        let mut env = BTreeMap::new();
        for key in &args[3..3 + count] {
            let key = key.to_str().context("non-UTF8 environment name")?;
            let value = std::env::var(key).context("forwarded environment missing")?;
            ensure!(
                env.insert(key.to_owned(), value).is_none(),
                "duplicate environment name"
            );
        }
        validate_environment(&env)?;
        let target = Path::new(&args[3 + count]);
        absolute_text(target)?;
        let error = std::process::Command::new(target)
            .args(&args[4 + count..])
            .env_clear()
            .envs(env)
            .exec();
        return Err(error.into());
    }
    #[cfg(not(unix))]
    bail!("epoch producer trampoline requires Unix");
}

/// Opaque, noncloneable completion proof. The lease-owning caller may consume it;
/// merely dropping a scope, timing out, or signalling a unit never produces one.
pub(crate) struct SettledProducer {
    _private: (),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProducerExit {
    Code(i32),
    Signal(i32),
}

#[derive(Clone, Debug)]
struct Manager {
    uid: u32,
    runner: PathBuf,
    control: PathBuf,
    helper: PathBuf,
}

impl Manager {
    fn new() -> Result<Self> {
        #[cfg(target_os = "linux")]
        let uid = unsafe { libc::getuid() };
        #[cfg(not(target_os = "linux"))]
        bail!("epoch containment requires Linux cgroup v2");
        fn locate(name: &str) -> Result<PathBuf> {
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|p| p.join(name))
                .find(|p| p.is_file())
                .context("systemd user-service tooling unavailable")?
                .canonicalize()
                .map_err(Into::into)
        }
        Ok(Self {
            uid,
            runner: locate("systemd-run")?,
            control: locate("systemctl")?,
            helper: std::env::current_exe()?,
        })
    }
    fn command(&self, executable: &Path) -> Command {
        let mut command = Command::new(executable);
        command
            .env("XDG_RUNTIME_DIR", format!("/run/user/{}", self.uid))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path=/run/user/{}/bus", self.uid),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    async fn show(&self, name: &str) -> Result<BTreeMap<String, String>> {
        let output = self.command(&self.control).args(["--user", "show", name,
            "--property=LoadState,ControlGroup,InvocationID,ExecMainCode,ExecMainStatus,ActiveState,SubState"]).output().await?;
        ensure!(
            output.status.success(),
            "user manager property query failed"
        );
        Ok(String::from_utf8(output.stdout)?
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect())
    }
}

#[derive(Clone, Debug)]
struct Unit {
    manager: Manager,
    name: String,
    invocation: String,
    cgroup: PathBuf,
}

fn bind_cgroup(uid: u32, value: &str) -> Result<PathBuf> {
    let prefix = format!("/user.slice/user-{uid}.slice/user@{uid}.service/");
    ensure!(
        value.starts_with(&prefix) && !value.contains("..") && !value.ends_with('/'),
        "unexpected producer cgroup"
    );
    Ok(Path::new("/sys/fs/cgroup").join(value.trim_start_matches('/')))
}

fn populated(path: &Path) -> Result<bool> {
    let events = std::fs::read_to_string(path.join("cgroup.events"))?;
    match events.lines().find_map(|l| l.strip_prefix("populated ")) {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => bail!("invalid cgroup.events"),
    }
}

impl Unit {
    async fn properties(&self) -> Result<BTreeMap<String, String>> {
        let p = self.manager.show(&self.name).await?;
        ensure!(
            p.get("InvocationID") == Some(&self.invocation),
            "unit identity lost or replaced"
        );
        Ok(p)
    }
    async fn completion(&self) -> Result<ProducerExit> {
        loop {
            let p = self.properties().await?;
            let code = p
                .get("ExecMainCode")
                .context("missing main code")?
                .parse::<i32>()?;
            let empty = match populated(&self.cgroup) {
                Ok(value) => !value,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    // The start is resolved and bound to this invocation. With
                    // ExitType=cgroup a terminal service (or RemainAfterExit's
                    // exited state) has no remaining family. Only THEN does a
                    // removed, previously bound cgroup establish completion.
                    terminal(&p)
                }
                Err(error) => return Err(error),
            };
            if code != 0 && empty {
                let status = p
                    .get("ExecMainStatus")
                    .context("missing main status")?
                    .parse()?;
                return match code {
                    1 => Ok(ProducerExit::Code(status)),
                    2 | 3 => Ok(ProducerExit::Signal(status)),
                    _ => bail!("unsupported ExecMainCode"),
                };
            }
            tokio::time::sleep(POLL).await;
        }
    }
    async fn stop(self) -> Result<()> {
        self.properties().await?; // Bind the stop to the recorded invocation, not a missing pending unit.
        let output = self
            .manager
            .command(&self.manager.control)
            .args(["--user", "stop", &self.name])
            .output()
            .await?;
        ensure!(output.status.success(), "owned producer stop failed");
        // A completed synchronous StopUnit job is a known terminal lifecycle.
        // Removal is accepted only here, never during pending start/collection.
        loop {
            match populated(&self.cgroup) {
                Ok(false) => break,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    break
                }
                Err(error) => return Err(error),
                Ok(true) => tokio::time::sleep(POLL).await,
            }
        }
        // A legitimate target exit 1 leaves a failed service object. Clear only
        // this never-reused, already-stopped unit so each ordinary Git refusal
        // does not introduce another unbounded manager-retention leak. The
        // journal is retained; this does not operate on any daemon service.
        let cleanup = self
            .manager
            .command(&self.manager.control)
            .args(["--user", "reset-failed", &self.name])
            .output()
            .await?;
        if !cleanup.status.success() {
            // A successful stopped unit may already have been collected. This
            // bookkeeping failure cannot invalidate observed family emptiness.
            eprintln!(
                "epoch producer unit cleanup: {}",
                String::from_utf8_lossy(&cleanup.stderr)
            );
        }
        Ok(())
    }
}

fn terminal(properties: &BTreeMap<String, String>) -> bool {
    matches!(
        properties.get("ActiveState").map(String::as_str),
        Some("inactive" | "failed")
    ) || properties.get("SubState").is_some_and(|s| s == "exited")
}

async fn launch(manager: Manager, recipe: EpochProducerCommand, runtime: Duration) -> Result<Unit> {
    validate_environment(&recipe.environment)?;
    absolute_text(&recipe.executable)?;
    absolute_text(&recipe.cwd)?;
    ensure!(
        recipe.argv.iter().all(|s| !s.contains('\0')),
        "NUL argument"
    );
    // Kernel-generated random identity, never reused even across daemon restarts.
    let mut random = [0u8; 24];
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let name = format!(
        "dracon-epoch-{}.service",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    // Bind the intended path before starting. A fast command can finish and
    // lose its ControlGroup property before the launch client returns; it must
    // not be mistaken for an unresolved start or rely on a guessed slice.
    let slice = manager.show("app.slice").await?;
    let base = bind_cgroup(
        manager.uid,
        slice.get("ControlGroup").context("missing app slice")?,
    )?;
    let expected_cgroup = base.join(&name);
    let mut command = manager.command(&manager.runner);
    command.env_clear().envs(&recipe.environment);
    for (key, value) in [
        ("XDG_RUNTIME_DIR", format!("/run/user/{}", manager.uid)),
        (
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path=/run/user/{}/bus", manager.uid),
        ),
    ] {
        ensure!(
            recipe.environment.get(key).is_none_or(|v| v == &value),
            "unsupported bus environment override"
        );
        command.env(key, value);
    }
    command.args([
        "--user",
        "--quiet",
        "--service-type=exec",
        "--expand-environment=no",
        "--unit",
        &name,
    ]);
    command.args([
        "--property=Slice=app.slice",
        "--property=ExitType=cgroup",
        "--property=RemainAfterExit=yes",
        "--property=KillMode=control-group",
        "--property=SendSIGKILL=yes",
        "--property=TimeoutStopSec=2s",
    ]);
    command.arg(format!("--property=RuntimeMaxSec={}s", runtime.as_secs()));
    command.arg(format!(
        "--working-directory={}",
        absolute_text(&recipe.cwd)?
    ));
    for (property, path) in [
        ("StandardInput", recipe.stdin.as_ref()),
        ("StandardOutput", recipe.stdout.as_ref()),
        ("StandardError", recipe.stderr.as_ref()),
    ] {
        command.arg(if let Some(path) = path {
            format!("--property={property}=file:{}", absolute_text(path)?)
        } else {
            format!("--property={property}=null")
        });
    }
    for key in recipe.environment.keys() {
        command.arg(format!("--setenv={key}"));
    }
    command
        .arg("--")
        .arg(&manager.helper)
        .arg(TRAMPOLINE)
        .arg(recipe.environment.len().to_string())
        .args(recipe.environment.keys())
        .arg(&recipe.executable)
        .args(&recipe.argv);
    // No timeout/kill_on_drop: the scope's registered task owns the pending start.
    // A client error is uncertain containment, never a target exit/refusal.
    let output = command.output().await?;
    ensure!(
        output.status.success(),
        "producer service launch unqualified: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let properties = manager.show(&name).await?;
    ensure!(
        properties.get("LoadState").is_some_and(|s| s == "loaded"),
        "producer disappeared during launch"
    );
    let invocation = properties
        .get("InvocationID")
        .context("missing invocation")?
        .clone();
    ensure!(
        invocation.len() == 32 && invocation.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid invocation"
    );
    let reported = properties
        .get("ControlGroup")
        .context("missing control group")?;
    let cgroup = if reported.is_empty() {
        ensure!(terminal(&properties), "unresolved producer cgroup");
        expected_cgroup
    } else {
        let observed = bind_cgroup(manager.uid, reported)?;
        ensure!(
            observed == expected_cgroup,
            "producer cgroup does not match owned unit"
        );
        observed
    };
    Ok(Unit {
        manager,
        name,
        invocation,
        cgroup,
    })
}

struct ScopeState {
    closed: bool,
    launches: Vec<JoinHandle<Result<Unit>>>,
}

/// Keep this scope outside the request timeout and alongside the spool lease.
/// Dropping it does NOT prove completion; finite service runtime is only a backstop.
pub(crate) struct EpochProducerScope {
    manager: Manager,
    runtime: Duration,
    state: Mutex<ScopeState>,
}

impl EpochProducerScope {
    pub(crate) fn new(runtime: Duration) -> Result<Self> {
        ensure!(
            (1..=3600).contains(&runtime.as_secs()),
            "producer runtime must be 1..3600 seconds"
        );
        Ok(Self {
            manager: Manager::new()?,
            runtime,
            state: Mutex::new(ScopeState {
                closed: false,
                launches: Vec::new(),
            }),
        })
    }
    /// Registration happens synchronously BEFORE returning the cancellable request.
    pub(crate) fn run(
        &self,
        recipe: EpochProducerCommand,
    ) -> Result<impl std::future::Future<Output = Result<ProducerExit>>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("producer state poisoned"))?;
        ensure!(!state.closed, "producer scope closed");
        let (tx, rx) = oneshot::channel();
        let manager = self.manager.clone();
        let runtime = self.runtime;
        state.launches.push(tokio::spawn(async move {
            let result = launch(manager, recipe, runtime).await;
            let _ = tx.send(result.as_ref().map(Clone::clone).map_err(|e| e.to_string()));
            result
        }));
        Ok(async move {
            rx.await
                .context("producer supervisor lost")?
                .map_err(anyhow::Error::msg)?
                .completion()
                .await
        })
    }
    /// Closing is irreversible. The settlement task itself also survives cancellation.
    /// Deadline expiry, pending starts, manager errors and survivors yield NO proof.
    pub(crate) async fn settle(self, deadline: Duration) -> Result<SettledProducer> {
        let mut state = self
            .state
            .into_inner()
            .map_err(|_| anyhow::anyhow!("producer state poisoned"))?;
        state.closed = true;
        let task = tokio::spawn(async move {
            let mut uncertain = Vec::new();
            for launch in state.launches {
                match launch.await {
                    Ok(Ok(unit)) => {
                        if let Err(e) = unit.stop().await {
                            uncertain.push(e.to_string());
                        }
                    }
                    other => uncertain.push(format!("unresolved producer launch: {other:?}")),
                }
            }
            ensure!(
                uncertain.is_empty(),
                "producer settlement unqualified: {}",
                uncertain.join("; ")
            );
            Ok(SettledProducer { _private: () })
        });
        tokio::time::timeout(deadline, task)
            .await
            .context("producer settlement deadline; retain spool")?
            .context("settlement supervisor failed")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_removals_and_literals() {
        let mut env = BTreeMap::from([
            ("GIT_DIR".into(), "wrong".into()),
            ("KEEP".into(), "$HOME literal".into()),
        ]);
        apply_changes(
            &mut env,
            [
                ("GIT_DIR".into(), None),
                ("GIT_INDEX_FILE".into(), Some("/tmp/index".into())),
            ],
        );
        assert!(!env.contains_key("GIT_DIR"));
        assert_eq!(env["KEEP"], "$HOME literal");
        assert_eq!(env["GIT_INDEX_FILE"], "/tmp/index");
        assert!(validate_environment(&BTreeMap::from([("BAD=KEY".into(), "x".into())])).is_err());
    }

    #[tokio::test]
    #[ignore = "requires user bus, cgroup v2 and prebuilt DRACON_EPOCH_TEST_HELPER; only disposable own units"]
    async fn host_descendants_pending_cancellation_and_exact_environment() -> Result<()> {
        let helper = PathBuf::from(std::env::var("DRACON_EPOCH_TEST_HELPER")?);
        ensure!(
            helper.is_absolute() && helper.is_file(),
            "build the normal helper first"
        );
        let directory = tempfile::tempdir()?;
        let shell = std::fs::canonicalize("/bin/sh")?;
        let setsid = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join("setsid"))
            .find(|p| p.is_file())
            .context("setsid unavailable")?
            .canonicalize()?;
        let output = directory.path().join("output");
        let mut scope = EpochProducerScope::new(Duration::from_secs(12))?;
        scope.manager.helper = helper.clone();
        let recipe = EpochProducerCommand {
            executable: shell.clone(), cwd: directory.path().to_owned(),
            argv: vec!["-c".into(), format!("printf '%s:%s' \"$KEEP\" \"${{GIT_DIR-unset}}\"; '{}' '{}' -c 'exec sleep 6' </dev/null >/dev/null 2>&1 & exit 1", setsid.display(), shell.display())],
            environment: BTreeMap::from([("PATH".into(), std::env::var("PATH")?), ("KEEP".into(), "$HOME literal".into())]),
            stdin: None, stdout: Some(output.clone()), stderr: None,
        };
        let request = scope.run(recipe)?;
        // Cancellation BEFORE polling the request cannot erase the registered launch.
        drop(request);
        let started = std::time::Instant::now();
        scope.settle(Duration::from_secs(20)).await?;
        ensure!(
            started.elapsed() < Duration::from_secs(20),
            "settlement was not bounded"
        );
        // A separate fixture observes leader status while closed-stdio setsid child
        // remains populated, then proves numeric exit 1 is not a wrapper error.
        let mut scope = EpochProducerScope::new(Duration::from_secs(12))?;
        scope.manager.helper = helper;
        let recipe = EpochProducerCommand {
            executable: shell, cwd: directory.path().to_owned(),
            argv: vec!["-c".into(), format!("printf '%s:%s' \"$KEEP\" \"${{GIT_DIR-unset}}\"; '{}' /bin/sh -c 'exec sleep 1' </dev/null >/dev/null 2>&1 & exit 1", setsid.display())],
            environment: BTreeMap::from([("PATH".into(), std::env::var("PATH")?), ("KEEP".into(), "$HOME literal".into())]),
            stdin: None, stdout: Some(output.clone()), stderr: None,
        };
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(15), scope.run(recipe)?).await?;
        let settlement = scope.settle(Duration::from_secs(20)).await;
        assert_eq!(result?, ProducerExit::Code(1));
        settlement?;
        ensure!(
            started.elapsed() >= Duration::from_millis(800),
            "escaped descendant lifetime"
        );
        assert_eq!(std::fs::read_to_string(output)?, "$HOME literal:unset");
        Ok(())
    }

    #[test]
    fn missing_cgroup_is_not_completion_of_a_pending_or_running_unit() {
        for substate in ["start", "start-pre", "running"] {
            let properties = BTreeMap::from([
                ("ActiveState".into(), "active".into()),
                ("SubState".into(), substate.into()),
            ]);
            assert!(!terminal(&properties));
        }
        for (state, substate) in [
            ("failed", "failed"),
            ("inactive", "dead"),
            ("active", "exited"),
        ] {
            let properties = BTreeMap::from([
                ("ActiveState".into(), state.into()),
                ("SubState".into(), substate.into()),
            ]);
            assert!(terminal(&properties));
        }
    }

    #[test]
    fn cgroup_binding_rejects_other_owners_and_traversal() {
        assert!(bind_cgroup(1000, "/system.slice/dracon.service").is_err());
        assert!(bind_cgroup(
            1000,
            "/user.slice/user-1001.slice/user@1001.service/app.slice/x"
        )
        .is_err());
        assert!(bind_cgroup(1000, "/user.slice/user-1000.slice/user@1000.service/../x").is_err());
        assert!(bind_cgroup(
            1000,
            "/user.slice/user-1000.slice/user@1000.service/app.slice/x.service"
        )
        .is_ok());
    }
}
