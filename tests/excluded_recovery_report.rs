//! Persistent private recovery must not be normalized healthy by a raw-clean report.
use std::{path::Path,process::Command,time::Duration};
fn git(repo:&Path,args:&[&str])->Vec<u8>{
    let mut command=Command::new("git");
    for(key,_)in std::env::vars_os(){if key.to_string_lossy().starts_with("GIT_"){command.env_remove(key);}}
    let out=command.arg("--no-optional-locks").arg("-C").arg(repo).args(args)
        .env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1").output().unwrap();
    assert!(out.status.success(),"{args:?}: {}",String::from_utf8_lossy(&out.stderr));out.stdout
}
async fn report(home:&Path,policy:&Path)->serde_json::Value{
    let mut cmd=tokio::process::Command::new(env!("CARGO_BIN_EXE_dracon-sync"));
    for(key,_)in std::env::vars_os(){if key.to_string_lossy().starts_with("GIT_"){cmd.env_remove(key);}}
    cmd.args(["repos","fixture-repo","--json"])
        .env("HOME",home).env("XDG_CONFIG_HOME",home.join("config")).env("XDG_STATE_HOME",home.join("state"))
        .env("DRACON_SYNC_POLICY",policy).env("DRACON_SYNC_STATE_DIR",home.join("sync-state"))
        .env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1").env_remove("DRACON_SYNC_GIT_BIN")
        .kill_on_drop(true);
    let out=tokio::time::timeout(Duration::from_secs(30),cmd.output()).await.expect("bounded report").unwrap();
    assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}
#[tokio::test]
async fn unresolved_private_recovery_is_a_machine_readable_concern_even_when_git_is_clean(){
    let d=tempfile::tempdir().unwrap();let repo=d.path().join("fixture-repo");std::fs::create_dir(&repo).unwrap();
    git(&repo,&["init","--template=","-q","-b","main"]);
    git(&repo,&["config","user.name","DraconDev"]);git(&repo,&["config","user.email","dracsharp@gmail.com"]);
    git(&repo,&["config","commit.gpgsign","false"]);
    std::fs::write(repo.join("tracked.txt"),b"baseline\n").unwrap();
    git(&repo,&["add","--","tracked.txt"]);git(&repo,&["commit","-q","-m","baseline"]);
    let bare=d.path().join("origin.git");git(&repo,&["init","--template=","--bare","-q",bare.to_str().unwrap()]);
    git(&repo,&["remote","add","origin",bare.to_str().unwrap()]);git(&repo,&["push","-q","-u","origin","main"]);
    let policy=d.path().join("policy.toml");
    let mut settings=toml::map::Map::new();
    settings.insert("watch_roots".into(),toml::Value::Array(vec![toml::Value::String(repo.to_str().unwrap().into())]));
    settings.insert("auto_github_private".into(),toml::Value::Boolean(false));
    settings.insert("remotes".into(),toml::Value::Array(vec![]));
    std::fs::write(&policy,toml::to_string(&settings).unwrap()).unwrap();
    let initial=report(d.path(),&policy).await;
    assert_eq!(initial["rows"][0]["concern"],false,"fixture starts without a convergence concern: {initial}");
    let pending=repo.join(".git/dracon-excluded-recovery-unknown");std::fs::create_dir(&pending).unwrap();
    std::fs::write(pending.join("recovery.json"),b"unresolved retained transaction\n").unwrap();
    let index=std::fs::read(repo.join(".git/index")).unwrap();
    assert!(git(&repo,&["status","--porcelain=v1","--untracked-files=all"]).is_empty());
    let observed=report(d.path(),&policy).await;
    let row=&observed["rows"][0];
    assert_eq!(row["concern"],true,"raw-clean cannot hide retained uncertainty: {observed}");
    assert!(row["state_flags"].as_array().unwrap().iter().any(|f|f=="EXCLUDED_RECOVERY"));
    assert_eq!(row["modified"],0);assert_eq!(row["staged"],0);assert_eq!(row["untracked"],0);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(),index,"report is read-only");
    assert_eq!(std::fs::read(pending.join("recovery.json")).unwrap(),b"unresolved retained transaction\n");
}
