# browser-extensions-shared: published to no remote since 2026-10-26

## Observation

`dracon-sync repos` reported the repo `AHEAD:1542`, `push_status: PENDING`,
`last_push: "32 hours ago"`, while continuing to commit (`commits_1h: 47`).
The daemon journal showed, repeatedly:

    ⏳ … committed; publication owned by another worker (this pass did not push)
    ⚠️ … epoch was published; deadline cancelled post-publication follow-up
    🚨 ALERT: … has 3259 unpushed commits (threshold: 50).

Read-only remote checks:

    local HEAD          8d4fd6cc
    github  main        45c33097   (== origin/main, 1542 behind local)
    gitlab  main        9a83787e   (an ancestor of local: simply behind)

So neither destination had EVER received the epoch the journal claimed to have
published, and no gate error, rejection or non-fast-forward was logged.

## Reproduced cause

`git push --dry-run gitlab HEAD:refs/heads/main` (publishes nothing, but runs the
repository's real configured pre-push hook) took **25m21s** and ended with:

    Connection to gitlab.com closed by remote host.

The hook runs TYPECHECK/BUILD/TEST/LINT for every affected workspace — 165 log
files in a normal attempt, 112 attempt directories retained for this repo — so
the TCP connection sits completely silent for ~25 minutes before the pack
transfer starts. The effective SSH configuration had:

    serveraliveinterval 0      <-- keepalive disabled
    serveralivecountmax 3
    tcpkeepalive yes

With keepalive off, the forge closes the idle connection during the gate, and
the push dies after all gates have already passed. That is why the "epoch was
published" line and "0 commits arrived" coexisted: the failure happened after
the gate succeeded and before anything was sent.

This is NOT a gate failure, NOT a rejection, and NOT non-fast-forward. No gate
was weakened, skipped or shortened to reach this conclusion, and the dry-run
did not publish anything.

## Repair (environment, reversible)

Added to `~/.ssh/config` `Host *`:

    ServerAliveInterval 30
    ServerAliveCountMax 10
    TCPKeepAlive yes

Verified effective: `ssh -G gitlab.com` now reports
`serveraliveinterval 30` / `serveralivecountmax 10`, and no `core.sshCommand`
override exists (global or repo), so the daemon's git uses this config.

Backup of the previous config (mode 0600, 685 bytes):
`/home/dracon/.dracon/utilities/ssh-config-backups/ssh-config-before-keepalive-20261027T215822Z`

Reversal: restore that file. Nothing else about the gates, hooks, timeouts or
policy was changed.

## Separate, unresolved condition in the same repo

The repository's pack is 3.67 GiB, above GitHub's 2 GiB hard limit, so the
GitHub leg cannot succeed regardless of keepalive. The daemon's pack guard
skips it. Resolving that needs asset migration out of Git history, which is
out of scope for this repair and is NOT claimed as fixed.
