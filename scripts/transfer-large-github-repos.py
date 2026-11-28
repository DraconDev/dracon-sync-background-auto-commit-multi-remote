#!/usr/bin/env python3
"""Move discovered private GitHub repositories that exceed 2 GiB of own Git storage."""

import argparse
from contextlib import ExitStack, contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import time
import tomllib

LIMIT_BYTES = 2 * 1024**3
GITHUB = re.compile(
    r"(?:git@github\.com:|ssh://git@github\.com/|https://github\.com/)"
    r"([\w-]+)/([\w.-]+?)(?:\.git)?", re.ASCII
)


def run(*args, missing=None):
    result = subprocess.run(args, capture_output=True, text=True, timeout=60)
    if result.returncode == 0:
        return result.stdout.rstrip("\n")
    if missing and missing(result):
        return None
    command = args[0] if isinstance(args[0], str) else str(args[0])
    raise RuntimeError(f"{command}: {result.stderr.strip() or result.returncode}")


def api(endpoint, *args, absent=False):
    output = run("gh", "api", endpoint, *args,
                 missing=(lambda result: "(HTTP 404)" in result.stderr) if absent else None)
    return json.loads(output) if output else None


def git(repo, *args, optional=False):
    return run("git", "-C", repo, *args, missing=(lambda result: result.returncode == 1)
               if optional else None)


def project(url):
    match = GITHUB.fullmatch(url or "")
    return "/".join(match.groups()).lower() if match else None


def own_bytes(repo):
    gitdir = Path(git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir"))
    objects = gitdir / "objects"
    loose = sum(
        blob.stat().st_size
        for directory in objects.iterdir()
        if re.fullmatch(r"[0-9a-f]{2}", directory.name) and directory.is_dir()
        for blob in directory.iterdir()
        if re.fullmatch(r"[0-9a-f]{38}|[0-9a-f]{62}", blob.name) and blob.is_file()
    )
    return loose + sum(pack.stat().st_size for pack in (objects / "pack").glob("*.pack"))


class Busy(RuntimeError):
    pass


@contextmanager
def lease(path):
    with os.fdopen(os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600), "r+") as lock:
        info = os.fstat(lock.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_nlink != 1:
            raise RuntimeError(f"unsafe lease file {path}")
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise Busy(f"{path.parent} is busy") from error
        yield


def repo_leases(stack, repo, publication=True):
    common = Path(git(repo, "rev-parse", "--path-format=absolute", "--git-common-dir"))
    if publication:
        stack.enter_context(lease(common / "dracon-sync.publication-lease"))
    stack.enter_context(lease(common / "dracon-sync.operation-lease"))


def replace(path, content):
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as tmp:
        temp = Path(tmp.name)
        try:
            if path.exists():
                os.fchmod(tmp.fileno(), stat.S_IMODE(path.stat().st_mode))
            tmp.write(content)
            tmp.flush()
            os.fsync(tmp.fileno())
            temp.replace(path)
            directory = os.open(path.parent, os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        finally:
            temp.unlink(missing_ok=True)


def override_plan(repo):
    path = repo / ".dracon/dracon-sync.toml"
    if path.is_symlink() or path.parent.is_symlink():
        raise RuntimeError("repository override is a symlink")
    source = path.read_text() if path.exists() else ""
    parsed = tomllib.loads(source)
    if parsed.get("owned") is False:
        raise RuntimeError("repository explicitly opts out of sync ownership")
    remotes = parsed.get("exclude_remotes", [])
    if not isinstance(remotes, list) or any(not isinstance(name, str) for name in remotes):
        raise RuntimeError("invalid exclude_remotes override")
    if "github" in remotes:
        return path, None
    target = "exclude_remotes = " + json.dumps([*remotes, "github"])
    if "exclude_remotes" in parsed:
        updated, count = re.subn(r"(?m)^exclude_remotes[ \t]*=[ \t]*\[[^\n]*\](?=[ \t]*(?:#|$))",
                                 lambda _: target, source)
        if count != 1:
            raise RuntimeError("update the multiline mirror override before transfer")
    else:
        updated = target + "\n" + source
    expected = dict(parsed, exclude_remotes=[*remotes, "github"])
    if tomllib.loads(updated) != expected:
        raise RuntimeError("repository override changed unexpectedly")
    return path, updated


def submodule_plan(parent, repo, allowed):
    if parent is None:
        return None
    rel = repo.relative_to(parent).as_posix()
    if (parent / ".gitmodules").is_symlink() or not (parent / ".gitmodules").is_file():
        raise RuntimeError("parent .gitmodules is missing or is a symlink")
    entries = git(parent, "config", "--file", ".gitmodules", "--get-regexp",
                  r"^submodule\..*\.path$", optional=True) or ""
    keys = [key[:-4] + "url" for key, _, value in
            (line.partition(" ") for line in entries.splitlines()) if value == rel]
    if len(keys) != 1:
        raise RuntimeError(f"cannot identify the parent submodule for {rel}")
    key = keys[0]
    tracked = git(parent, "config", "--file", ".gitmodules", "--get-all", key)
    local = git(parent, "config", "--local", "--get-all", key, optional=True)
    if project(tracked) not in allowed or (local and project(local) not in allowed):
        raise RuntimeError(f"unexpected submodule URL for {rel}")
    return key


def check_remotes(repo, allowed):
    for remote in ("origin", "github"):
        raw = git(repo, "config", "--local", "--get-all", f"remote.{remote}.url", optional=True)
        if raw is None and remote != "origin":
            continue
        urls = [raw, git(repo, "remote", "get-url", "--all", remote),
                git(repo, "remote", "get-url", "--push", "--all", remote)]
        if any(project(url) not in allowed for url in urls):
            raise RuntimeError(f"{remote} has a custom URL that needs manual cutover")
        if git(repo, "config", "--get-all", f"remote.{remote}.pushurl", optional=True):
            raise RuntimeError(f"{remote} has an explicit push URL that needs manual cutover")
    if git(repo, "config", "--get-regexp", r"^(lfs\.url|lfs\.pushurl|remote\..*\.lfs(push)?url)$",
           optional=True) or (repo / ".lfsconfig").exists():
        raise RuntimeError("explicit LFS configuration needs manual cutover")


def source_eligible(meta, old):
    return (meta["full_name"].lower() == old and meta["owner"]["type"] == "User"
            and meta["visibility"] == "private" and meta["permissions"]["admin"]
            and not any(meta.get(key) for key in ("archived", "disabled", "fork", "has_pages")))


def transfer(repo, old, options, journal=None):
    destination = options.destination
    slug = old.split("/", 1)[1]
    if old.split("/", 1)[0] != options.source.lower():
        raise RuntimeError("transfer journal is outside the configured source account")
    target = f"{destination}/{slug}"
    new = target.lower()
    canonical = f"git@github.com:{target}.git"
    parent_path = git(repo, "rev-parse", "--show-superproject-working-tree")
    parent = Path(parent_path) if parent_path else None
    with ExitStack() as stack:
        if options.apply:
            if parent:
                repo_leases(stack, parent, publication=False)
            repo_leases(stack, repo)
        allowed = {old, new} if journal else {old}
        check_remotes(repo, allowed)
        key = submodule_plan(parent, repo, allowed)
        override, content = override_plan(repo)
        if journal:
            record = json.loads(journal.read_text())
            meta = api(f"repositories/{record['id']}")
        else:
            if own_bytes(repo) <= LIMIT_BYTES:
                return
            meta = api(f"repos/{old}")
            if not source_eligible(meta, old):
                return
            branch = meta["default_branch"]
            sha = api(f"repos/{old}/git/ref/heads/{branch}")["object"]["sha"]
            record = dict(repo=str(repo), old=old, target=new, id=meta["id"], branch=branch, sha=sha)
            journal = options.state / f"{meta['id']}.json"

        current = meta["full_name"].lower()
        if (record["repo"], record["old"], record["target"]) != (str(repo), old, new):
            raise RuntimeError("transfer journal does not match policy and checkout")
        if current == old:
            if not source_eligible(meta, old):
                raise RuntimeError("source repository is no longer eligible")
            if api(f"repos/{target}", absent=True) is not None:
                raise RuntimeError(f"{target} already exists")
        elif current != new or meta["owner"]["type"] != "Organization":
            raise RuntimeError(f"repository ID resolved to unexpected owner {current}")

        if not options.apply:
            print(f"{old} → {target} ({record['id']}, {repo})")
            return
        if not journal.exists():
            replace(journal, json.dumps(record) + "\n")
        if content is not None:
            override.parent.mkdir(exist_ok=True)
            relative = ".dracon/dracon-sync.toml"
            if not (git(repo, "ls-files", "--", relative) or
                    git(repo, "check-ignore", "--", relative, optional=True)):
                exclude = Path(git(repo, "rev-parse", "--path-format=absolute",
                                   "--git-path", "info/exclude"))
                exclude.parent.mkdir(exist_ok=True)
                previous = exclude.read_text() if exclude.exists() else ""
                replace(exclude, previous + "\n/.dracon/dracon-sync.toml\n")
            replace(override, content)
        if current == old:
            if time.time() - record.get("attempted_at", 0) < 1800:
                raise Busy(f"{target} is waiting for GitHub to complete the transfer")
            record["attempted_at"] = int(time.time())
            replace(journal, json.dumps(record) + "\n")
            api(f"repos/{old}/transfer", "--method", "POST", "-f", f"new_owner={destination}")
        moved = api(f"repos/{target}", absent=True)
        if not moved or moved["disabled"]:
            raise Busy(f"{target} is waiting for GitHub to complete the transfer")
        if (moved["id"] != record["id"] or moved["full_name"].lower() != new
                or moved["visibility"] != "private" or moved["default_branch"] != record["branch"]):
            raise RuntimeError(f"{target} failed identity, visibility or default-branch verification")
        branch = record["branch"]
        sha = api(f"repos/{target}/git/ref/heads/{branch}")["object"]["sha"]
        ssh_ref = git(repo, "ls-remote", "--exit-code", canonical, f"refs/heads/{branch}").split()
        if ssh_ref != [sha, f"refs/heads/{branch}"]:
            raise Busy(f"{target} API and Git ref differ during cutover")
        if sha != record["sha"]:
            ancestry = api(f"repos/{target}/compare/{record['sha']}...{sha}")
            if ancestry["merge_base_commit"]["sha"] != record["sha"]:
                raise RuntimeError(f"{target} lost the branch tip recorded before transfer")
        for remote in ("github", "origin"):
            if git(repo, "config", "--local", "--get", f"remote.{remote}.url", optional=True):
                git(repo, "remote", "set-url", remote, canonical)
        if key:
            git(parent, "config", "--file", ".gitmodules", key, canonical)
            git(parent, "config", "--local", key, canonical)
        journal.unlink()
        print(f"Transferred {old} → {target} (GitHub ID {record['id']}, {branch} {sha[:12]})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--destination", required=True)
    parser.add_argument("--sync", default="dracon-sync")
    parser.add_argument("--state", type=Path,
                        default=Path.home() / ".local/state/dracon-sync/github-transfers")
    parser.add_argument("--apply", action="store_true", help="enable ownership changes")
    options = parser.parse_args()
    if (not all(re.fullmatch(r"[A-Za-z0-9-]+", owner) for owner in
                (options.source, options.destination))
            or options.source.lower() == options.destination.lower()):
        parser.error("provide distinct GitHub account and organization names")
    options.state.mkdir(mode=0o700, parents=True, exist_ok=True)
    with lease(options.state / "transfer.lock"):
        user = api("user")["login"]
        membership = api(f"user/memberships/orgs/{options.destination}")
        if (user.lower() != options.source.lower() or membership["role"] != "admin"
                or membership["state"] != "active"):
            raise RuntimeError("GitHub auth must be the source account and a destination organization admin")

        errors = 0
        pending = set()
        for journal in sorted(options.state.glob("*.json")):
            record = json.loads(journal.read_text())
            repo = Path(record["repo"])
            pending.add(repo)
            try:
                transfer(repo, record["old"], options, journal)
            except Busy as error:
                print(f"Deferred {repo}: {error}", file=sys.stderr)
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                errors += 1
                print(f"Transfer needs attention for {repo}: {error}", file=sys.stderr)

        rows = json.loads(run(options.sync, "ownership", "--json"))["results"]
        for row in sorted(rows, key=lambda row: (-len(Path(row["repo"]).parts), row["repo"])):
            repo = Path(row["repo"])
            old = project(row["origin_url"])
            if (repo in pending or row["report"]["kind"] != "owned"
                    or not old or old.split("/")[0] != user.lower()):
                continue
            try:
                if own_bytes(repo) > LIMIT_BYTES:
                    transfer(repo, old, options)
            except Busy as error:
                print(f"Deferred {repo}: {error}", file=sys.stderr)
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                errors += 1
                print(f"Transfer needs attention for {repo}: {error}", file=sys.stderr)
        return bool(errors)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Busy as error:
        print(f"Deferred: {error}", file=sys.stderr)
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        sys.exit(str(error))
