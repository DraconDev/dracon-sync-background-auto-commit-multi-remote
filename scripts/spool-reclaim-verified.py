#!/usr/bin/env python3
"""Verified epoch-spool reclaim (operator maintenance; run under `dracon-sync maintenance --`).

The daemon's lossless reclaim is currently UNQUALIFIED
(`LOSSLESS_RECLAIM_QUALIFIED = false` in src/git/epoch_spool.rs) and
`repair spools --apply` correctly preserves everything. This script is the
operator-approved maintenance path instead:

  1. Enumerate auditable spools: <gitdir>/dracon-staging-* in the given repos.
  2. Prove duplication: every object in each spool's retained packs must
     already exist in the main object store (`git cat-file --batch-all-objects`).
  3. Bundle-first: any spool whose pack holds objects MISSING from the main
     store has that pack copied (and integrity-verified) into the backup dir
     with a JSON manifest before anything is deleted.
  4. Delete only spools proven fully duplicated (or whose missing objects are
     now bundled). Skips marker files `preserve-rejected` / `original-index`
     (the daemon's protected() set) unconditionally.

Dry-run by default. `--apply` + `--backup-dir` required for deletion.
Intended use: `dracon-sync maintenance -- python3 scripts/spool-reclaim-verified.py --repo ... --apply --backup-dir ...`
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time

PROTECTED_MARKERS = ("preserve-rejected", "original-index")


def main_objects(repo: str) -> set[str]:
    out = subprocess.run(
        ["git", "-C", repo, "cat-file", "--batch-all-objects", "--batch-check=%(objectname)"],
        capture_output=True, text=True, check=True,
    )
    return set(out.stdout.split())


def pack_objects(repo: str, pack: str) -> list[str]:
    out = subprocess.run(
        ["git", "verify-pack", "-v", pack],
        capture_output=True, text=True, check=False,
    )
    if out.returncode != 0:
        # No objects listed etc. Treat as unreadable → never delete.
        raise RuntimeError(f"verify-pack failed for {pack}: {out.stderr.strip()}")
    objects = []
    for line in out.stdout.splitlines():
        if len(line) >= 40:
            try:
                oid = line.split()[0]
            except IndexError:
                continue
            if all(c in "0123456789abcdef" for c in oid) and len(oid) == 40:
                objects.append(oid)
    return objects


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--repo", required=True, help="watched repo worktree")
    ap.add_argument("--apply", action="store_true", help="actually delete (default: dry-run)")
    ap.add_argument("--backup-dir", default="", help="required with --apply")
    args = ap.parse_args()

    if args.apply and not args.backup_dir:
        print("error: --apply requires --backup-dir", file=sys.stderr)
        return 2

    gitdir = subprocess.run(
        ["git", "-C", args.repo, "rev-parse", "--path-format=absolute", "--git-dir"],
        capture_output=True, text=True, check=True,
    ).stdout.strip()

    spools = sorted(
        d for d in os.listdir(gitdir)
        if d.startswith("dracon-staging-") and os.path.isdir(os.path.join(gitdir, d))
    )
    print(f"{args.repo}: {len(spools)} spool dir(s); apply={args.apply}")

    main_set = main_objects(args.repo)
    print(f"main object store: {len(main_set)} object(s)")

    report = {"repo": args.repo, "started": time.time(), "deletable": 0, "bundled": 0,
              "skipped": 0, "missing_objects": 0, "bytes_freed": 0}

    if args.apply:
        os.makedirs(args.backup_dir, exist_ok=True)

    for name in spools:
        spool = os.path.join(gitdir, name)
        if any(os.path.lexists(os.path.join(spool, m)) for m in PROTECTED_MARKERS):
            print(f"  SKIP  {name}: protected marker present")
            report["skipped"] += 1
            continue
        packs = [os.path.join(spool, f) for f in os.listdir(spool)
                 if f.startswith("retained-") and f.endswith(".pack")]
        if not packs:
            # No retained pack: index-only spool. Its objects were never
            # independently packed; verify the index's objects are in main.
            print(f"  SKIP  {name}: no retained pack (index-only; manual review)")
            report["skipped"] += 1
            continue
        size = sum(os.path.getsize(os.path.join(spool, f)) for f in os.listdir(spool)
                   if os.path.isfile(os.path.join(spool, f)))
        missing: set[str] = set()
        for pack in packs:
            try:
                missing |= {o for o in pack_objects(args.repo, pack) if o not in main_set}
            except RuntimeError as exc:
                print(f"  SKIP  {name}: {exc}")
                report["skipped"] += 1
                missing = None
                break
        if missing is None:
            continue
        if missing:
            if not args.apply:
                print(f"  BUNDLE {name}: {len(missing)} object(s) missing from main store")
                report["bundled"] += 1
                report["missing_objects"] += len(missing)
                continue
            dest = os.path.join(args.backup_dir, name)
            os.makedirs(dest, exist_ok=True)
            with open(os.path.join(dest, "manifest.json"), "w") as fh:
                json.dump({"spool": name, "missing": sorted(missing),
                           "packs": [os.path.basename(p) for p in packs]}, fh)
            for pack in packs:
                shutil.copy2(pack, os.path.join(dest, os.path.basename(pack)))
            for pack in packs:
                copied = os.path.join(dest, os.path.basename(pack))
                if pack_objects(args.repo, copied) == [] and os.path.getsize(copied) == 0:
                    print(f"  SKIP  {name}: bundled pack unreadable after copy")
                    report["skipped"] += 1
                    missing = None
                    break
            if missing is None:
                continue
            print(f"  BUNDLE {name}: {len(missing)} object(s) bundled to {dest}")
            report["bundled"] += 1
            report["missing_objects"] += len(missing)
        else:
            print(f"  DELETE {name}: fully duplicated ({size} bytes)")
        if args.apply:
            shutil.rmtree(spool)
            report["deletable"] += 1
            report["bytes_freed"] += size

    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
