# The one remaining blocker needs an owner decision (not an agent workaround)

## State

Fleet: **35 ok / 0 warn / 0 concern / 0 failures**, zero ahead, zero pending —
up from 16 ok / 15 warn with two repositories unable to publish at all.

Two repositories are still DIRTY, and both are one decision away from clean.

## The blocker, identified precisely

`junk-runner` cannot commit because the **Warden machine-global** pre-commit
hook (`~/.config/git/hooks/pre-commit`) refuses three files that are staged:

    .pi-glla/session-owner.json
    .pi-glla/update-check.json
    .pi-glla/pending-approval-renders.json

These are NOT newly added by an agent: all three are already TRACKED in HEAD
(`git cat-file -e HEAD:.pi-glla/session-owner.json` succeeds) and are staged as
modifications. The hook lists them in a hardcoded "never overridden by an audit
inclusion" category and says so explicitly:

    Confirm the intended content/ignore policy; preserve local bytes.
    Source/evidence permission requires scoped root rules; frame caches remain blocked.
    Index-only removal is not a durable fix for deliberately included content.

`dracon-platform` is dirty only through gitlink pointers to this repository and
to `hegemon`, which the daemon deliberately quarantines in `exclude_repos`.

## Why this was not "fixed" by the agent

Every available shortcut is forbidden by the objective or by the hook itself:

- **Untracking the three files** (`git rm --cached`) — the hook names this as
  "not a durable fix for deliberately included content", and removing tracked
  state to force a green observation is exactly "hiding work".
- **Bypassing the hook** (`--no-verify`) — explicitly forbidden.
- **Editing the global Warden policy** to exempt them — that weakens a security
  guard for every repository on the machine, to unblock one.
- **Stopping the agents that edit these repos** — explicitly forbidden.

The hook's own wording ("Confirm the intended content/ignore policy") describes
a human confirmation. That is the decision.

## What the owner decides

Should these three machine-local runtime/state files be (a) tracked
deliberately — in which case the hook needs a scoped allowance for this
repository, or (b) untracked with a scoped root `.gitignore` rule, which is what
the hook calls the durable fix?

Either answer unblocks `junk-runner`, and with it `dracon-platform`'s gitlink
dirtiness. Nothing else in the fleet is waiting on it.
