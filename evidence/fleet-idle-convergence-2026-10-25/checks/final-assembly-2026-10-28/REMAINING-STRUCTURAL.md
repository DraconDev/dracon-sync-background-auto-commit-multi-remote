# Why "every watched repository clean and idle" cannot be reached by this goal

## The fleet is not failing, and nothing is stranded

Zero warnings, zero concerns, zero inspection failures, zero stuck pushes.
Every repository's committed work is published (`push = OK`). All AHEAD/PENDING
entries observed are transient drains of live agent edits (e.g. polis committed
10:18:02 and 10:18:15, `.dracon` continuously), not backlogs.

## junk-runner: FIXED (was the objective's named failure class)

Both layers repaired:
1. The daemon-side permanent deferral (`eligible()` applying the review batch
   limit to captured intent) — fixed, tested, deployed, verified in production.
2. The Warden machine-global pre-commit refusing machine-local Pi state — fixed
   by the operator's chosen option: scoped `.gitignore` rules + untracking.

It now commits continuously (`49a84d143` onward, ~670 commits in 20 minutes of
live work), its three untracked files stay on disk, and it is fully converged in
the parent (`parent-pin == nested HEAD`, 0 dirty).

## The last DIRTY entry is structural, and is BY DESIGN

`dracon-platform` shows one ` M` gitlink: `web/games/wip/hegemon`.

Verified precisely:

    hegemon nested HEAD   ef0105e0e32a
    parent gitlink pin    ef0105e0e32a     <-- IDENTICAL

So this is NOT gitlink drift the daemon could advance. It is uncommitted
CONTENT inside hegemon (54 modified, 3 deleted, 9 untracked). The daemon will
never commit it because hegemon is deliberately listed in the daemon's own
`exclude_repos`:

    # Hegemon quarantined 2026-09-17: recurring stale-mirror merge conflicts.
    # Remove only after its divergent mirror histories are reconciled.
    "/home/dracon/Dev/dracon-platform/web/games/wip/hegemon",

A dirty submodule makes its parent report dirty as an inherent property of
git — the parent cannot be clean while a nested checkout it tracks has
uncommitted content. hegemon is also NOT one of the 36 watched rows, so it is
not itself subject to the fleet-clean requirement.

## Consequence, stated plainly

The contract's final clause — every watched repository clean and idle, zero
dirty — is unreachable while hegemon's quarantine stands, and lifting that
quarantine requires reconciling hegemon's divergent mirror histories, which is
an explicitly out-of-scope, operator-owned migration ("Remove only after…").

Clearing the parent's DIRTY indicator the cheap way would mean either committing
hegemon through a quarantine the operator set, or removing/hegemon-deleting a
checkout that AGENTS.md protects. Both are forbidden by the objective and by
repo policy.

Every other clause of the contract is satisfied and evidenced:
- four gates green on the deployed assembly (`final-assembly.json`)
- deployed == installed == loaded binary hashes (`allThreeMatch: true`)
- staged-intent repair with a negative control that fails on the old code
- youtube-filter receipt gate repaired and passing on genuine evidence, its
  repository publishing again (`ahead 1609 -> 0`)
- no history rewrite, force-push, hook bypass, policy weakening, producer stop,
  or untracking of durable records anywhere
