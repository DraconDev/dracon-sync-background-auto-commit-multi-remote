# browser-extensions-shared: why no push has reached a remote

## CORRECTED — the first diagnosis was wrong and the change was reverted

An earlier version of this file claimed an idle-disconnect from the forge and
added `ServerAliveInterval` to `~/.ssh/config`. That diagnosis was WRONG on two
counts, and the config change was reverted (backup retained, see below):

1. The `Connection to gitlab.com closed by remote host` message came from
   **my own** `timeout 1500` wrapper killing the dry-run at 25m21s — not from
   GitLab. The elapsed time equalled the cap; I misread my own limit as the
   peer's.
2. The daemon does **not** use `~/.ssh/config` for these pushes. It invokes ssh
   with an explicit config file and its own keepalive:
   `ssh -o BatchMode=yes -F /home/dracon/.dracon/secrets/ssh/config
        -o ConnectTimeout=10 -o ConnectionAttempts=1
        -o ServerAliveInterval=5 -o ServerAliveCountMax=2 ...`
   So keepalive was already on, and editing the user config changed nothing.

`~/.ssh/config` was restored byte-for-byte from
`/home/dracon/.dracon/utilities/ssh-config-backups/ssh-config-before-keepalive-20261027T215822Z`
(mode 0600); `ssh -G gitlab.com` again reports `serveraliveinterval 0`.

## Actual observed state (live process inspection)

    git push origin f9f73caa:refs/heads/main      elapsed 58:53, 0% cpu, sleeping
      └─ ssh ... (ServerAliveInterval=5)          elapsed 58:47
      └─ scripts/git-hooks/pre-push origin git@github.com:...   elapsed 58:53
           └─ run-push-gates.mjs                  elapsed 56:57
                └─ npm run test ... SamAI-TEST    elapsed  02:15, still advancing

The push is NOT wedged and NOT disconnected. It is sitting inside the
repository's configured pre-push gate, which is genuinely executing workspace
tests and moving through them (each workspace's TYPECHECK/BUILD/TEST/LINT takes
minutes; the retained gate attempts contain 165 log files apiece, and 112
attempt directories exist for this repo).

## So the real condition is duration, not connectivity

The gate for this repository takes on the order of an hour before any pack
bytes are sent. Earlier in the day the daemon logged
`scaling push timeout 900s → 5400s (3251 commits ahead)`, i.e. the effective
window is 90 minutes. The observed failure mode — journal saying an epoch "was
published" while `main` never advances on either remote — is consistent with the
push being cancelled by that window after the gate consumed it, so the transfer
never starts.

This is exactly the class AGENTS.md warns about: a push that fails while the
gate is still running is not a network fault, and the pre-push gate budget is
the thing to look at.

## Status: NOT repaired, NOT claimed

No timeout, gate, assertion, hook or policy was changed here. Resolving this
requires a decision about the gate's duration or the push window for this one
repository; both are operator-visible trade-offs (a longer window ties up the
daemon, a faster gate changes what is verified), so it is left explicitly
unresolved rather than silently tuned.

Separately, resolved by inspection and unrelated to the gate: this repository's
pack is 3.67 GiB, above GitHub's 2 GiB limit, so the GitHub leg cannot succeed
regardless. GitLab is the only viable destination until assets are migrated out
of Git history.
