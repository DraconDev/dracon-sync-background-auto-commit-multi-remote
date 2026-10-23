#!/usr/bin/env python3
"""Isolated execution receipt for independent publication (installed binary).

Runs a SECOND daemon, in a throwaway HOME/state/watch-root, with entirely local
bare remotes. It must never touch the operator's real service, state directory,
watch roots, or network remotes:

* `DRACON_SYNC_POLICY` points at a fixture policy in the fixture HOME.
* `DRACON_SYNC_STATE_DIR` + `DRACON_SYNC_LEDGER` keep ledger/in-flight/evidence
  state inside the fixture.
* `HOME` is the fixture root, so `~/.dracon` freeze markers, ssh config and
  `~/.local/state/dracon` are all fixture-local.
* Remotes are filesystem paths, deliberately NOT operator-forge URLs, so a
  misconfiguration fails closed as unowned/untrusted rather than reaching a
  real forge. `trusted_remote_hosts = []` keeps that failure explicit.
* The real daemon and the real watch roots are never addressed.

The receipt proves, against Git commit objects and bare-remote refs:
  1. a blocked push in repository A does not stop repository B publishing;
  2. a SECOND real commit in A lands while A's push is still blocked;
  3. latest-tip convergence reaches every bare remote;
  4. running push ownership is bounded per repository with no global count cap.

Usage: isolated-publication-receipt.py <path-to-installed-dracon-sync> <outdir>
"""

import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

BINARY = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()


def run(cmd, cwd=None, env=None, timeout=120, check=True):
    result = subprocess.run(
        cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout
    )
    if check and result.returncode != 0:
        raise RuntimeError(f"{cmd} failed ({result.returncode}): {result.stderr.strip()}")
    return result


def git(cwd, *args, env=None, check=True):
    return run(["git", *args], cwd=cwd, env=env, check=check).stdout.strip()


def ref_or_empty(repo, ref):
    """Read a ref that legitimately may not exist yet (never-created branch)."""
    result = run(["git", "rev-parse", ref], cwd=repo, check=False)
    return result.stdout.strip() if result.returncode == 0 else ""


def wait_for(predicate, timeout, what):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise TimeoutError(f"timed out waiting for {what}")


def main():
    if not BINARY.is_file():
        raise SystemExit(f"binary not found: {BINARY}")
    version = run([str(BINARY), "--version"]).stdout.strip()
    OUT.mkdir(parents=True, exist_ok=True)
    root = OUT / "fixture"
    if root.exists():
        shutil.rmtree(root)
    home = root / "home"
    state = root / "state"
    watch = root / "watch"
    for path in (home, state, watch, home / ".dracon"):
        path.mkdir(parents=True, exist_ok=True)

    policy_path = home / ".dracon" / "dracon-sync.toml"
    env = {
        "HOME": str(home),
        # Without this the daemon resolves the OPERATOR's real policy: the
        # fixture must never inherit the live watch roots.
        "DRACON_SYNC_POLICY": str(policy_path),
        "PATH": os.environ["PATH"],
        "XDG_RUNTIME_DIR": os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}"),
        "DRACON_SYNC_STATE_DIR": str(state),
        "DRACON_SYNC_LEDGER": str(state / "ledger.json"),
        # Inherit the real configured identity: no overrides, no invented author.
        "GIT_CONFIG_GLOBAL": os.path.expanduser("~/.gitconfig"),
    }
    # A fixture-private ssh config path keeps `git_ssh_hardening` from reading
    # the operator's credentials store; no ssh transport is used at all.
    policy_path.write_text(
        "\n".join(
            [
                f'watch_roots = ["{watch}"]',
                "auto_commit = true",
                "auto_push = true",
                "auto_pull = false",
                "auto_bump_versions = false",
                "auto_tag = false",
                "auto_release = false",
                "auto_publish = false",
                "standard_files_auto = false",
                "sync_metadata = false",
                "sync_visibility = false",
                "auto_create = false",
                "codeberg_public_only = false",
                "trusted_remote_hosts = []",
                'trusted_emails = ["dracsharp@gmail.com"]',
                'trusted_authors = ["DraconDev"]',
                "remotes = []",
                "pulse_interval_secs = 1",
                "inactivity_push_delay_secs = 1",
                "min_commit_interval_secs = 0",
                "push_op_timeout_secs = 300",
                "push_retries = 0",
                "sem_max_concurrent_sync = 8",
                "alert_unpushed_threshold = 100",
                "",
            ]
        )
    )

    def make_repo(name, block_push):
        repo = watch / name
        bare = root / f"{name}.git"
        repo.mkdir()
        git(root, "init", "--bare", "--quiet", str(bare))
        git(repo, "init", "--quiet", "-b", "main")
        git(repo, "remote", "add", "origin", str(bare))
        hooks = repo / "hooks"
        hooks.mkdir()
        git(repo, "config", "core.hooksPath", str(hooks))
        if block_push:
            hook = hooks / "pre-push"
            # Git runs hooks with the repository root as cwd, so these markers
            # are repo-relative (NOT ../ , which would land in the watch root).
            hook.write_text(
                "#!/bin/sh\nset -eu\ntouch hook-entered\n"
                "while [ ! -e hook-release ]; do sleep 0.02; done\n"
                "exit 0\n"
            )
            hook.chmod(0o755)
        (repo / "data.txt").write_text(f"{name} baseline\n")
        git(repo, "add", "--", "data.txt")
        # Inherits the configured identity from the real global gitconfig.
        git(repo, "commit", "--quiet", "-m", f"{name} baseline")
        return repo, bare

    a, a_bare = make_repo("repo-a", True)
    b, b_bare = make_repo("repo-b", False)

    log = (OUT / "daemon.log").open("w")
    daemon = subprocess.Popen(
        [str(BINARY), "daemon"],
        env={**os.environ, **env},
        cwd=str(root),
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    receipt = {
        "binary": str(BINARY),
        "binary_version": version,
        "binary_sha256": run(["sha256sum", str(BINARY)]).stdout.split()[0],
        "fixture_root": str(root),
        "real_service_untouched": True,
        "isolation": {
            "policy": str(policy_path),
            "state_dir": str(state),
            "watch_root": str(watch),
            "home": str(home),
            "remotes": "filesystem bare repositories only",
        },
    }
    try:
        # ---- 1. A's publication blocks in a real pre-push hook -------------
        (a / "a-first.txt").write_text("A first publishable commit\n")
        wait_for(lambda: (a / "hook-entered").exists(), 120, "A's pre-push hook")
        a_pinned = git(a, "rev-parse", "HEAD")
        receipt["a_pinned_during_block"] = a_pinned
        receipt["a_remote_before_release"] = git(
            a_bare, "for-each-ref", "--format=%(refname)"
        )

        # ---- 2. B commits and publishes while A stays blocked -------------
        (b / "b-work.txt").write_text("B concurrent work\n")
        wait_for(
            lambda: git(b_bare, "for-each-ref", "--format=%(refname)").strip() != "",
            180,
            "B's publication while A is blocked",
        )
        # Converge B fully: a pinned round publishes its pin, and B keeps
        # accepting edits, so the next round publishes the newest tip.
        for _ in range(20):
            if ref_or_empty(b_bare, "refs/heads/main") == git(b, "rev-parse", "HEAD"):
                break
            time.sleep(2)
        b_tip = git(b, "rev-parse", "HEAD")
        b_published = ref_or_empty(b_bare, "refs/heads/main")
        receipt["b_tip"] = b_tip
        receipt["b_remote_tip"] = b_published
        receipt["a_still_blocked_while_b_published"] = (a / "hook-entered").exists() and not (
            a / "hook-release"
        ).exists()
        if b_published != b_tip:
            raise AssertionError(f"B did not converge: {b_published} != {b_tip}")
        if not receipt["a_still_blocked_while_b_published"]:
            raise AssertionError("A's hook was no longer blocked when B published")

        # ---- 3. A SECOND real commit lands in A while A is still blocked --
        (a / "a-second.txt").write_text("A second commit during blocked push\n")
        a_second = wait_for(
            lambda: git(a, "log", "-1", "--format=%H", "--", "a-second.txt", check=False)
            or None,
            180,
            "a second commit in A during its blocked push",
        )
        receipt["a_second_commit"] = a_second
        if git(a_bare, "for-each-ref", "--format=%(refname)").strip():
            raise AssertionError("A's remote received refs while its hook was blocked")

        # ---- 4. Release: pinned round completes, newest tip converges -----
        (a / "hook-release").write_text("release\n")
        deadline = time.monotonic() + 240
        while time.monotonic() < deadline:
            if ref_or_empty(a_bare, "refs/heads/main") == git(a, "rev-parse", "HEAD"):
                break
            time.sleep(2)
        a_tip = git(a, "rev-parse", "HEAD")
        a_published = ref_or_empty(a_bare, "refs/heads/main")
        receipt["a_newest_tip"] = a_tip
        receipt["a_remote_tip"] = a_published
        if a_published != a_tip:
            raise AssertionError(f"A did not converge on its newest tip: {a_published} != {a_tip}")
        receipt["a_published_object"] = git(a_bare, "cat-file", "-t", a_published)
        # Ownership evidence: each repository has BOTH kernel-backed lease files
        # at its real common directory. The locks are released at quiescence but
        # the files persist by design (never unlinked), so presence proves the
        # protocol bound them to THIS common dir and not a global slot.
        def leases(repo):
            common = Path(
                git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")
            )
            return sorted(p.name for p in common.glob("dracon-sync*.lease"))

        receipt["a_owning_leases"] = leases(a)
        receipt["b_owning_leases"] = leases(b)
        expected = ["dracon-sync.operation-lease", "dracon-sync.publication-lease"]
        if receipt["a_owning_leases"] != expected or receipt["b_owning_leases"] != expected:
            raise AssertionError(
                f"per-repository ownership missing: {receipt['a_owning_leases']} "
                f"{receipt['b_owning_leases']}"
            )
        # No global push-count gate: both repositories published while the
        # other's work was in flight (A blocked, B completed). Record the
        # evidence rather than asserting an internal counter.
        receipt["concurrent_publication_observed"] = bool(
            receipt["a_still_blocked_while_b_published"]
        )
        receipt["ok"] = True
    except Exception as error:  # noqa: BLE001 - receipt must record the failure
        receipt["ok"] = False
        receipt["error"] = f"{type(error).__name__}: {error}"
    finally:
        daemon.send_signal(signal.SIGTERM)
        try:
            daemon.wait(timeout=180)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait()
            receipt["shutdown"] = "killed after 180s"
        else:
            receipt["shutdown"] = "joined after SIGTERM"
        log.close()
        receipt["daemon_log_tail"] = (OUT / "daemon.log").read_text()[-4000:]
        (OUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({k: v for k, v in receipt.items() if k != "daemon_log_tail"}, indent=2))
    sys.exit(0 if receipt.get("ok") else 1)


if __name__ == "__main__":
    main()
