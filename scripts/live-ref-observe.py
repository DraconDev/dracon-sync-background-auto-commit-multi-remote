#!/usr/bin/env python3
"""Read-only live remote-ref observation for the publication goal.

Records, per representative repository: local HEAD, each remote's advertised
ref (via `git ls-remote`, which contacts the remote but changes nothing), and
whether the local tip exists as a real commit object (`git cat-file -e`).

No push, no fetch, no ref update, no config change: every command used here is
a pure read. The purpose is to prove that remote refs ADVANCE for the installed
daemon revision and that what advanced is a genuine commit object, rather than
inferring progress from status fields.
"""
from __future__ import annotations

import json
import subprocess
import sys
import time

REPOS = [
    "/home/dracon/Dev/browser-extensions-shared",
    "/home/dracon/Dev/dracon-platform",
    "/home/dracon/.dracon",
    "/home/dracon/Dev/dracon-utilities",
    "/home/dracon/Dev/dracon-utilities/dracon-sync",
]


def run(args, cwd=None):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=180)


def git_out(repo, *args):
    result = run(["git", *args], cwd=repo)
    if result.returncode != 0:
        return f"<error: {result.stderr.strip()[:200]}>"
    return result.stdout.strip()


def observe(repo):
    head = git_out(repo, "rev-parse", "HEAD")
    branch = git_out(repo, "rev-parse", "--abbrev-ref", "HEAD")
    remotes = git_out(repo, "remote").splitlines()
    record = {"repo": repo, "branch": branch, "head": head, "remotes": {}}
    for remote in remotes:
        url = git_out(repo, "remote", "get-url", remote)
        ls = run(["git", "ls-remote", remote, f"refs/heads/{branch}"], cwd=repo)
        ref = ""
        if ls.returncode == 0 and ls.stdout.strip():
            ref = ls.stdout.split()[0]
        exists = False
        kind = ""
        if ref:
            cat = run(["git", "cat-file", "-t", ref], cwd=repo)
            if cat.returncode == 0:
                kind = cat.stdout.strip()
                exists = kind == "commit"
        record["remotes"][remote] = {
            "url": url,
            "query_succeeded": ls.returncode == 0,
            "query_error": ls.stderr.strip()[:500] if ls.returncode else "",
            "branch_ref": ref,
            "remote_tip_is_real_commit_object": exists,
            "object_type": kind,
            "in_sync_with_local_head": bool(ref) and ref == head,
        }
    return record


def main():
    label = sys.argv[1] if len(sys.argv) > 1 else "snapshot"
    pid = run(["systemctl", "show", "dracon-sync-operator.service", "-p", "MainPID", "--value"]).stdout.strip()
    loaded = ""
    if pid:
        exe = run(["sha256sum", f"/proc/{pid}/exe"]).stdout.split()
        loaded = exe[0] if exe else ""
    payload = {
        "label": label,
        "observed_at": int(time.time()),
        "daemon_pid": pid,
        "loaded_exe_sha256": loaded,
        "repos": [],
    }
    for repo in REPOS:
        try:
            payload["repos"].append(observe(repo))
        except Exception as error:  # noqa: BLE001 - observation must not abort
            payload["repos"].append({"repo": repo, "error": str(error)})
    print(json.dumps(payload, indent=2))


if __name__ == "__main__":
    main()
