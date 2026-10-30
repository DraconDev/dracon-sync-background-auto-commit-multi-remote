#!/usr/bin/env python3
"""Independently re-derive fleet convergence from PER-ROW state.

ADDED 2026-10-30 after the auditor found that
`checks/fleet-observation-2026-10-29/*` had been accepted on the
report's own `ok` total, while the rows in those same files carried
DIRTY / AHEAD / PENDING work. `ok` used to be a residual bucket
(`rows - (concern+active+warn)`), so such rows were counted as
"clean".

This script therefore does NOT trust any summary integer. It derives
the answer from each row's own fields, and then CHECKS the report's
summary against that independent derivation. Disagreement is a hard
failure, not a warning: a report that disagrees with its own rows is
the defect being guarded against.

A row is CONVERGED only when ALL of these hold:
  * modified == 0            (no eligible tracked dirt)
  * staged == 0              (no staged intent)
  * untracked == 0           (no eligible untracked file)
  * excluded_dirty == 0      (intentionally-excluded dirt still on disk
                              is still work; it is merely work the
                              daemon will not commit)
  * ahead == 0               (nothing unpushed)
  * push_status == "OK"      (not PENDING / BLOCKED / BROKEN)
  * concern / warn / active  all false
  * no "DIRTY" or "AHEAD:<n>" state flag

Usage:
    verify-fleet-observation.py <observation.json> [--expect-repos N]
Exit: 0 all samples converged and internally consistent, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import sys


def row_converged(r: dict) -> bool:
    """The single definition of 'clean' — mirrors row_is_converged()."""
    if r.get("modified", 0) != 0:
        return False
    if r.get("staged", 0) != 0:
        return False
    if r.get("untracked", 0) != 0:
        return False
    if r.get("excluded_dirty", 0) != 0:
        return False
    if r.get("ahead", 0) != 0:
        return False
    if r.get("push_status") != "OK":
        return False
    if r.get("concern") or r.get("warn") or r.get("active"):
        return False
    for f in r.get("state_flags") or []:
        if f == "DIRTY" or f.startswith("AHEAD:"):
            return False
    return True


def explain(r: dict) -> str:
    """Why a row is not converged — the evidence the auditor needs."""
    why = []
    for key in ("modified", "staged", "untracked", "excluded_dirty", "ahead"):
        if r.get(key, 0):
            why.append(f"{key}={r[key]}")
    ps = r.get("push_status")
    if ps != "OK":
        why.append(f"push_status={ps}")
    for key in ("concern", "warn", "active"):
        if r.get(key):
            why.append(key)
    for f in r.get("state_flags") or []:
        if f == "DIRTY" or f.startswith("AHEAD:"):
            why.append(f)
    return ",".join(why)


def check(payload: dict, expect_repos: int | None) -> list[str]:
    problems: list[str] = []
    rows = payload.get("rows") or []
    repos = payload.get("repos")

    if expect_repos is not None and repos != expect_repos:
        problems.append(f"expected {expect_repos} repos, report covers {repos}")
    if len(rows) != repos:
        problems.append(f"row count {len(rows)} != repos {repos}")

    derived = {name: 0 for name in ("ok", "pending", "active", "warn", "concern")}
    offenders = []
    for r in rows:
        if row_converged(r):
            derived["ok"] += 1
            continue
        offenders.append(r)
        if r.get("concern"):
            derived["concern"] += 1
        elif r.get("active") and not r.get("concern"):
            derived["active"] += 1
        elif r.get("warn") and not r.get("active") and not r.get("concern"):
            derived["warn"] += 1
        else:
            derived["pending"] += 1

    total = sum(derived.values())
    if total != len(rows):
        problems.append(f"buckets sum {total} != rows {len(rows)}")

    # The report's own summary must MATCH the independent derivation.
    for name in ("ok", "pending", "active", "warn", "concern"):
        claimed = payload.get(name)
        if claimed is None:
            problems.append(f"report omits '{name}' (cannot audit it)")
        elif claimed != derived[name]:
            problems.append(
                f"report claims {name}={claimed} but per-row state gives {derived[name]}"
            )

    if payload.get("failures"):
        problems.append(f"{payload['failures']} inspection failure(s)")

    # An observation only proves convergence if EVERY row converged.
    if derived["ok"] != len(rows):
        problems.append(
            f"{len(offenders)} of {len(rows)} rows are NOT converged: "
            + "; ".join(
                f"{r['repo'].split('/')[-1]} [{explain(r)}]" for r in offenders[:8]
            )
            + ("" if len(offenders) <= 8 else f" (+{len(offenders) - 8} more)")
        )
    return problems


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--expect-repos", type=int, default=None)
    ap.add_argument("--summary", action="store_true", help="print derived buckets")
    args = ap.parse_args()

    failed = 0
    for path in args.files:
        with open(path, encoding="utf-8") as fh:
            payload = json.load(fh)
        problems = check(payload, args.expect_repos)
        rows = payload.get("rows") or []
        conv = sum(1 for r in rows if row_converged(r))
        if args.summary:
            print(
                f"{path}: repos={payload.get('repos')} converged={conv} "
                f"claimed_ok={payload.get('ok')} pending={payload.get('pending')} "
                f"active={payload.get('active')} warn={payload.get('warn')} "
                f"concern={payload.get('concern')} failures={payload.get('failures')}"
            )
        if problems:
            failed += 1
            print(f"FAIL {path}")
            for p in problems:
                print(f"  - {p}")
        elif not args.summary:
            print(f"OK   {path}  ({conv}/{len(rows)} repositories converged and idle)")

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
