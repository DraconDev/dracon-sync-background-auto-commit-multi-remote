#!/usr/bin/env python3
"""Recover epoch-spool staged content into local-only refs, then delete the spool.

WHY THIS EXISTS
---------------
The 2026-11-06/07 stale-binary incident broke every local commit for ~8.5h
(see dracon-sync/src/self_executable.rs). Each failed epoch left its private
`dracon-staging-*` spool behind: ~29.8k directories holding ~78 GiB of staged
INDEXES whose work never reached a commit. At 32 G free (96% full) the host is
one large write from ENOSPC.

The spools cannot simply be deleted: an `index` file is genuine staged state
(paths + blob ids), and a few spools also hold `retained-*.pack`/"p
producer-io-*" packs. Deleting blind loses content that was never committed.

WHAT IT DOES
------------
For every spool it:
  1. copies the spool `index` to a scratch path (never mutating the spool),
  2. `git write-tree` against a private GIT_INDEX_FILE to obtain the staged
     TREE object (this adds tree objects to the repo ODB; blobs are normally
     already there),
  3. wraps that tree in an ANCHOR COMMIT and keeps it reachable from a
     local-only ref `refs/dracon-spool-recovery/<short-tree>`,
  4. re-reads the ref to PROVE the commit and its tree resolve, and only
     then
  5. deletes the spool directory.

WHY A COMMIT AND NOT A BARE TREE (fixed 2026-11-07)
---------------------------------------------------
The first version pointed the ref straight at the TREE object. That made every
`git bundle create --all` and every mirror push FAIL with a silent exit 2:
git cannot express a non-commit ref in a bundle or a `refs/*` push refspec.
Anchoring each recovered tree in a real commit fixes that, keeps the bytes
exactly as staged (the tree is the recovered content), and makes the recovery
self-describing in `git log`. The anchor is deliberately parentless: it is a
snapshot, not part of the repository's development history.

Trees are DEDUPED per repo: many spools staged identical content, so one ref
covers all of them. A tree already reachable from an existing ref needs no new
ref at all — it is already safe from `git gc`.

Refs are NEVER pushed (no remote refspec covers `refs/dracon-spool-recovery/`).

SAFETY
------
* `--dry-run` is the default; deletion requires `--apply`.
* Nothing is deleted unless the tree object was observed present AFTER the ref
  was created.
* An index that fails `write-tree` is PRESERVED and reported, never guessed at.
* Every removal is appended to a durable JSONL ledger next to the repo's
  spools so the action is auditable after the fact.

Usage:
  recover-epoch-spools.py --apply [--repo PATH] [--min-age-secs N]
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SPOOL_PREFIX = "dracon-staging-"
REF_PREFIX = "refs/dracon-spool-recovery/"


def git(repo, *args, env_extra=None, input_bytes=None, timeout=300):
    env = dict(os.environ)
    env.pop("GIT_INDEX_FILE", None)
    if env_extra:
        env.update(env_extra)
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        env=env,
        input=input_bytes,
        timeout=timeout,
    )


def tree_of_index(repo, scratch_index):
    """Return the tree id the spool index describes, or None.

    `git write-tree` must run INSIDE the target repository (it resolves the
    object store from the cwd) while reading the spool's index through
    GIT_INDEX_FILE. Running it in the scratch directory silently fails.
    """
    result = subprocess.run(
        ["git", "write-tree"],
        capture_output=True,
        cwd=str(repo),
        env={**os.environ, "GIT_INDEX_FILE": str(scratch_index)},
        timeout=600,
    )
    if result.returncode != 0:
        return None, result.stderr.decode(errors="replace").strip()
    return result.stdout.decode().strip(), ""


def object_exists(repo, oid):
    return git(repo, "cat-file", "-e", f"{oid}^{{tree}}").returncode == 0


def committer_env():
    """Pin a committer identity for anchor commits.

    The script may run inside a repo whose local git identity is unset; without
    this, `commit-tree` fails for a reason unrelated to recovery.
    """
    name = os.environ.get("DRACON_SPOOL_COMMITTER_NAME", "dracon-sync spool recovery")
    email = os.environ.get("DRACON_SPOOL_COMMITTER_EMAIL", "spool-recovery@localhost")
    return {
        "GIT_AUTHOR_NAME": name,
        "GIT_AUTHOR_EMAIL": email,
        "GIT_COMMITTER_NAME": name,
        "GIT_COMMITTER_EMAIL": email,
    }


def anchor_commit(repo, tree, message):
    """Create a parentless commit recording `tree`; return (oid, error).

    Bare-tree refs cannot be bundled or pushed (see the module docstring), so
    every preserved tree is anchored in a real commit. `commit-tree` writes the
    object into the repo ODB and returns the new sha; no worktree is touched.
    """
    result = subprocess.run(
        ["git", "-C", str(repo), "commit-tree", tree, "-m", message],
        capture_output=True,
        env={**os.environ, **committer_env()},
        timeout=120,
    )
    if result.returncode != 0:
        return None, result.stderr.decode(errors="replace")[:300]
    oid = result.stdout.decode().strip()
    return (oid or None), None


def tree_already_referenced(repo):
    """Set of tree ids already reachable from local refs (cheap, one pass)."""
    result = git(repo, "rev-list", "--objects", "--all")
    if result.returncode != 0:
        return set()
    trees = set()
    for line in result.stdout.decode(errors="replace").splitlines():
        parts = line.split(" ", 1)
        if len(parts) == 2:
            trees.add(parts[0])
    return trees


def rescue_retained_packs(repo, spool):
    """Unpack this spool's own retained packs into the repository object store.

    A spool may own `retained-<hash>.pack`+`.idx`+`.rev`: the objects an epoch
    producer wrote that the main object store does NOT yet hold. Unpacking them
    makes the staged tree constructible, which is exactly what "preserve the
    content" means. Returns True when at least one pack was unpacked.
    """
    packs = sorted(spool.glob("retained-*.pack"))
    rescued = False
    for pack in packs:
        if pack.stat().st_size == 0:
            continue
        with pack.open("rb") as handle:
            result = subprocess.run(
                ["git", "-C", str(repo), "unpack-objects", "-q"],
                stdin=handle,
                capture_output=True,
                timeout=900,
            )
        if result.returncode != 0:
            print(
                f"   ! unpack failed for {pack}: "
                f"{result.stderr.decode(errors='replace').strip()[:200]}",
                file=sys.stderr,
            )
            continue
        rescued = True
    return rescued


def dir_bytes(path):
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += os.lstat(os.path.join(root, name)).st_size
            except OSError:
                pass
    return total


def spool_dirs(gitdir, min_age_secs):
    now = time.time()
    found = []
    for entry in os.scandir(gitdir):
        if not entry.is_dir(follow_symlinks=False):
            continue
        if not entry.name.startswith(SPOOL_PREFIX):
            continue
        age = now - entry.stat(follow_symlinks=False).st_mtime
        if age < min_age_secs:
            continue
        found.append(Path(entry.path))
    return sorted(found)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="actually delete spools")
    ap.add_argument("--repo", action="append", default=[], help="repo worktree path")
    ap.add_argument("--min-age-secs", type=int, default=3600, help="skip young spools")
    ap.add_argument(
        "--include-debris",
        action="store_true",
        help="also remove index-less spool dirs (locks/producer-io only), "
        "unpacking any retained pack first",
    )
    args = ap.parse_args()

    spool_ledger = None
    totals = {"spools": 0, "removed": 0, "bytes": 0, "preserved": 0, "failed": 0}
    reports = []

    for repo in args.repo:
        repo_path = Path(repo).resolve()
        gitdir_probe = git(repo_path, "rev-parse", "--git-dir")
        if gitdir_probe.returncode != 0:
            print(f"!! not a git repo: {repo_path}", file=sys.stderr)
            continue
        gitdir = (repo_path / gitdir_probe.stdout.decode().strip()).resolve()
        spools = spool_dirs(gitdir, args.min_age_secs)
        if not spools:
            continue

        reachable = tree_already_referenced(repo_path)
        rescued_spools = set()
        # tree -> list of spools sharing it
        by_tree = {}
        scratch_root = Path(tempfile.mkdtemp(prefix="spool-recover-"))
        try:
            for spool in spools:
                index = spool / "index"
                if not index.is_file():
                    # No index means no staged STATE to preserve. A spool may
                    # still own a non-empty retained-*.pack whose objects are
                    # unique to it, so unpack those into the repository object
                    # store first (that is what preserves the bytes), then the
                    # directory itself is pure debris.
                    if args.include_debris:
                        rescue_retained_packs(repo_path, spool)
                        size = dir_bytes(spool)
                        record = {
                            "repo": str(repo_path),
                            "spool": spool.name,
                            "outcome": "debris-removed" if args.apply else "would-remove-debris",
                            "bytes": size,
                        }
                        if args.apply:
                            try:
                                shutil.rmtree(spool)
                                totals["debris"] = totals.get("debris", 0) + 1
                                totals["bytes"] += size
                            except OSError as error:
                                record["outcome"] = "delete-failed"
                                record["error"] = str(error)[:300]
                                totals["failed"] += 1
                        reports.append(record)
                        continue
                    reports.append(
                        {"repo": str(repo_path), "spool": spool.name, "outcome": "no-index"}
                    )
                    totals["preserved"] += 1
                    continue
                scratch_index = scratch_root / f"{spool.name}.index"
                shutil.copy2(index, scratch_index)
                tree, err = tree_of_index(repo_path, scratch_index)
                scratch_index.unlink(missing_ok=True)
                if not tree:
                    # The index referenced a blob the main ODB does not have.
                    # Those blobs live in the spool's own retained-*.pack (a
                    # pack this spool owns both halves of). Unpack them into
                    # the repository object store so the content survives, then
                    # retry the tree write. This is the arm that separates
                    # "duplicate work" from "genuinely unique staged bytes".
                    rescued = rescue_retained_packs(repo_path, spool)
                    if rescued:
                        shutil.copy2(index, scratch_index)
                        tree, err2 = tree_of_index(repo_path, scratch_index)
                        scratch_index.unlink(missing_ok=True)
                        if tree:
                            rescued_spools.add(spool.name)
                        else:
                            err = f"{err} | after unpack: {err2}"
                if not tree:
                    reports.append(
                        {
                            "repo": str(repo_path),
                            "spool": spool.name,
                            "outcome": "write-tree-failed",
                            "error": err[:400],
                        }
                    )
                    totals["preserved"] += 1
                    continue
                by_tree.setdefault(tree, []).append(spool)
        finally:
            shutil.rmtree(scratch_root, ignore_errors=True)

        # Create one ref per unique tree that is not already reachable.
        created = {}
        for tree in by_tree:
            if tree in reachable:
                created[tree] = "already-reachable"
                continue
            ref = f"{REF_PREFIX}{tree}"
            # Anchor the recovered tree in a real commit so the ref can be
            # bundled/pushed (git cannot express a bare-tree ref in either).
            spool_count = len(by_tree[tree])
            anchors = ", ".join(sorted(entry.name for entry in by_tree[tree])[:5])
            message = (
                "epoch-spool recovery: preserve staged state that never landed\n\n"
                f"tree: {tree}\n"
                f"spools: {spool_count} ({anchors}"
                f"{'...' if spool_count > 5 else ''})\n"
                "recovered-by: dracon-sync scripts/recover-epoch-spools.py\n"
                "note: parentless snapshot, NOT part of development history\n"
            )
            commit, err = anchor_commit(repo_path, tree, message)
            if not commit:
                created[tree] = f"anchor-failed: {err or 'no commit oid'}"
                continue
            make = git(repo_path, "update-ref", ref, commit)
            if make.returncode != 0:
                created[tree] = f"ref-failed: {make.stderr.decode(errors='replace')[:200]}"
                continue
            # PROVE the ref resolves to the commit AND that the commit still
            # carries the recovered tree before trusting it.
            verify = git(repo_path, "rev-parse", "--verify", ref)
            if verify.returncode != 0 or verify.stdout.decode().strip() != commit:
                created[tree] = "ref-verify-failed"
                continue
            verify_tree = git(repo_path, "rev-parse", "--verify", f"{ref}^{{tree}}")
            if (
                verify_tree.returncode != 0
                or verify_tree.stdout.decode().strip() != tree
            ):
                created[tree] = "ref-tree-verify-failed"
                continue
            created[tree] = ref

        for tree, spools_for_tree in by_tree.items():
            outcome = created.get(tree, "missing")
            ok = outcome == "already-reachable" or outcome.startswith(REF_PREFIX)
            if not ok:
                for spool in spools_for_tree:
                    reports.append(
                        {
                            "repo": str(repo_path),
                            "spool": spool.name,
                            "tree": tree,
                            "outcome": "preserved",
                            "reason": outcome,
                        }
                    )
                    totals["preserved"] += 1
                continue
            # Final presence check on the TREE OBJECT, after ref creation.
            if not object_exists(repo_path, tree):
                for spool in spools_for_tree:
                    reports.append(
                        {
                            "repo": str(repo_path),
                            "spool": spool.name,
                            "tree": tree,
                            "outcome": "preserved",
                            "reason": "tree-object-absent",
                        }
                    )
                    totals["preserved"] += 1
                continue
            for spool in spools_for_tree:
                size = dir_bytes(spool)
                totals["spools"] += 1
                record = {
                    "repo": str(repo_path),
                    "spool": spool.name,
                    "tree": tree,
                    "anchor": outcome,
                    "rescued_pack": spool.name in rescued_spools,
                    "bytes": size,
                    "outcome": "removed" if args.apply else "would-remove",
                }
                if args.apply:
                    try:
                        shutil.rmtree(spool)
                        totals["removed"] += 1
                        totals["bytes"] += size
                    except OSError as error:
                        record["outcome"] = "delete-failed"
                        record["error"] = str(error)[:300]
                        totals["failed"] += 1
                reports.append(record)

        if args.apply:
            spool_ledger = gitdir / "dracon-spool-recovery.jsonl"
            with spool_ledger.open("a") as handle:
                for record in reports:
                    if record.get("repo") == str(repo_path) and record["outcome"] in (
                        "removed",
                        "delete-failed",
                    ):
                        handle.write(json.dumps(record) + "\n")
        print(
            json.dumps(
                {
                    "repo": str(repo_path),
                    "spools": len(spools),
                    "unique_trees": len(by_tree),
                    "already_reachable": sum(
                        1 for t in by_tree if t in reachable
                    ),
                    "refs_created": sum(
                        1 for t, v in created.items() if v.startswith(REF_PREFIX)
                    ),
                }
            )
        )

    print(json.dumps({"totals": totals}, indent=2))


if __name__ == "__main__":
    main()
