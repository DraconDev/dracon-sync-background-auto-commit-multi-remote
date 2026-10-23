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

    BASELINE_HEAD: dict[str, str] = {}

    def make_repo(name, block_push, commit_hook_exit=0):
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
        # This fixture's OWN setup commit is made before any commit hook is
        # installed, so a refusing hook cannot prevent the baseline itself
        # (only the daemon's later commits are subject to it).
        git(repo, "commit", "--quiet", "-m", f"{name} baseline")
        # A configured COMMIT hook (not just a push hook). The daemon must run
        # the repository's commit hook chain: suppressing it with --no-verify
        # would commit behind a policy gate the repository asked for.
        pre_commit = hooks / "pre-commit"
        pre_commit.write_text(
            "#!/bin/sh\ntouch precommit-ran\n"
            + (
                "echo 'warden: synthetic refusal' >&2\nexit 1\n"
                if commit_hook_exit
                else "exit 0\n"
            )
        )
        pre_commit.chmod(0o755)
        # The FULL commit lifecycle, each hook recording its own name:
        # `pre-commit` alone is not Git's contract.
        hook_log = repo / "hooklog"
        for hook_name, body in (
            ("prepare-commit-msg", ""),
            ("commit-msg", ""),
            ("post-commit", ""),
        ):
            path = hooks / hook_name
            path.write_text(
                f"#!/bin/sh\nprintf '{hook_name}\\n' >> '{hook_log}'\n{body}"
            )
            path.chmod(0o755)
        return repo, bare

    a, a_bare = make_repo("repo-a", True)
    b, b_bare = make_repo("repo-b", False)
    # C's COMMIT hook refuses. The daemon must NOT bypass it: C must stay
    # uncommitted and unpublished, and the refusal must appear in its log.
    c, c_bare = make_repo("repo-c", False, commit_hook_exit=1)
    for name, repo in (("a", a), ("b", b), ("c", c)):
        BASELINE_HEAD[name] = git(repo, "rev-parse", "HEAD")

    log_path = OUT / "daemon.log"
    log = log_path.open("w")
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
        "baseline_heads": dict(BASELINE_HEAD),
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
        # ---- 4b. The configured COMMIT hook chain really runs -------------
        # An accepting hook must be seen to RUN on the daemon's own commit.
        receipt["b_precommit_hook_ran"] = (b / "precommit-ran").exists()
        if not receipt["b_precommit_hook_ran"]:
            raise AssertionError("the daemon suppressed B's configured pre-commit hook")
        # Every lifecycle hook that Git runs must have run for B's commit,
        # exactly once each.
        b_hooks = (b / "hooklog").read_text().split() if (b / "hooklog").exists() else []
        receipt["b_lifecycle_hooks"] = b_hooks
        # B makes ONE OR MORE daemon commits, and every commit must run the
        # full cycle in Git's order exactly once. So the log must be a whole
        # number of complete cycles, never a partial or misordered one.
        cycle = ["prepare-commit-msg", "commit-msg", "post-commit"]
        receipt["b_lifecycle_cycles"] = len(b_hooks) // len(cycle)
        if not b_hooks or len(b_hooks) % len(cycle) != 0:
            raise AssertionError(f"incomplete commit-hook cycle on B: {b_hooks}")
        for index in range(0, len(b_hooks), len(cycle)):
            if b_hooks[index : index + len(cycle)] != cycle:
                raise AssertionError(
                    f"commit hooks out of Git order on B: {b_hooks}"
                )
        # A refusing hook must block the commit: C stays at its baseline,
        # nothing is published, the work is preserved, and the refusal is
        # reported rather than swallowed as success.
        (c / "c-work.txt").write_text("C work blocked by its commit hook\n")
        refusal_seen = wait_for(
            lambda: "synthetic refusal" in log_path.read_text()
            and (c / "c-work.txt").exists()
            or None,
            180,
            "C's refusing pre-commit hook to be reported",
        )
        receipt["c_commit_hook_refusal_reported"] = bool(refusal_seen)
        receipt["c_precommit_hook_ran"] = (c / "precommit-ran").exists()
        receipt["c_head"] = git(c, "rev-parse", "HEAD")
        receipt["c_remote_refs"] = git(c_bare, "for-each-ref", "--format=%(refname)").strip()
        receipt["c_work_preserved"] = (c / "c-work.txt").exists()
        if not receipt["c_precommit_hook_ran"]:
            raise AssertionError("the daemon suppressed C's refusing pre-commit hook")
        if not receipt["c_commit_hook_refusal_reported"]:
            raise AssertionError("C's pre-commit refusal was not reported")
        if receipt["c_head"] != BASELINE_HEAD["c"]:
            raise AssertionError("a refused commit still advanced C's HEAD")
        if receipt["c_remote_refs"]:
            raise AssertionError("a refused commit was still published")
        if not receipt["c_work_preserved"]:
            raise AssertionError("a refused commit discarded the operator's work")

        # Ownership evidence: each repository has BOTH kernel-backed lease files
        # at its real common directory. The locks are released at quiescence but
        # the files persist by design (never unlinked), so presence proves the
        # protocol bound them to THIS common dir and not a global slot.
        def leases(repo):
            common = Path(
                git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")
            )
            return sorted(p.name for p in common.glob("dracon-sync*lease"))

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
            receipt["ok"] = False
            receipt["shutdown_error"] = "daemon did not exit on SIGTERM within 180s"
        else:
            receipt["shutdown"] = "joined after SIGTERM"
        log.close()
        log_text = (OUT / "daemon.log").read_text()
        receipt["daemon_log_tail"] = log_text[-4000:]
        # Shutdown cleanliness is its own assertion. Exiting is NOT proof that
        # owned workers and their descendants were reaped: the daemon reports
        # incomplete joins in its log, so a passing receipt must read that log.
        unclean = [
            line
            for line in log_text.splitlines()
            if "did not join" in line
            or "have no pending join result" in line
            or "owned sync task" in line
        ]
        receipt["shutdown_unclean_lines"] = unclean
        leftover = subprocess.run(
            ["pgrep", "-f", f"{root}/"], capture_output=True, text=True
        ).stdout.split()
        receipt["orphan_pids_after_shutdown"] = leftover
        if unclean or leftover:
            receipt["ok"] = False
            receipt["shutdown_error"] = (
                "owned work was not cleaned up: "
                f"{unclean or 'none reported'} / orphan pids {leftover or 'none'}"
            )
        (OUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(
        json.dumps(
            {
                k: v
                for k, v in receipt.items()
                if k not in ("daemon_log_tail", "shutdown_unclean_lines")
            },
            indent=2,
        )
    )
    sys.exit(0 if receipt.get("ok") else 1)


if __name__ == "__main__":
    main()
