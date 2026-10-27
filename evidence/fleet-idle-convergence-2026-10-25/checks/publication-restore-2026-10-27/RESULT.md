# Publication containment lifted — result

## What was done

The global `auto_push = false` containment (2026-10-27) was lifted after its own
written release condition ("Restore only after fail-closed publication is
qualified") was met. Nothing else about the policy changed; the only edit was
that one line plus a comment naming the evidence.

## Safety preconditions verified BEFORE lifting it

Read-only, on all 18 ahead repositories:

- `ahead > 0`, `behind = 0` for every one → all fast-forwardable, no rewrite or
  force-push needed.
- Every pending range scanned for newly-added sensitive paths: **0** `.env*`,
  `*.pem`, `*.key` files added.
- Every pending range scanned for provider-shaped secrets in ADDED lines
  (`sk-proj-…`, `sk-…`, `ghp_…`, `AKIA…`, JWT `eyJ….….`): **0** across all 18,
  including browser-extensions-shared's 1535-commit range.
- `pack_too_large = false` for all; largest pack 7.92 GiB (dracon-platform) is
  on GitLab/GitHub where only browser-extensions-shared's 3.67 GiB exceeds the
  GitHub 2 GiB limit.
- The deployed pre-push hook was already verified fail-closed
  (`cc7caa7b…`, 7 scan checks, refuses real secrets, passes harmless work) with
  `fail_open_reproduced: false`.

## Observed result (autonomous, no manual pushes)

    before restore : ok=16 active=5 warn=15 concern=0 failures=0
    after  restore : ok=30 active=5 warn=1  concern=0 failures=0

17 repositories drained from ahead>0 to ahead=0 through the daemon alone:
freeport, dracon-sync, dracon-warden, db-gateway, deathrun, empire,
dracon-strategy, dracon-system, capture-anime-girls, pi-plugins, polis,
pi-goal-list-loop-audit, doomtap, endless-td, ai-auto-video, `.dracon`.

Destination verification (read-only `ls-remote` of each repo's actual remotes,
matching the repo's local HEAD): **32 of 33** clean repos matched on every
intended remote. The single mismatch was `polis`, where BOTH remotes agree with
each other and local had simply gained a commit after the push — active
editing, not a publication failure.

Note on remote naming: three repos (dracon-sync, empire,
pi-goal-list-loop-audit) publish GitHub through `origin`, not a remote literally
named `github`. An earlier probe that assumed a `github` remote reported them as
"MISSING"; that was a probe error, corrected, and all three match on `origin`.

## The one repository that did not converge

`browser-extensions-shared` remains ahead and is NOT claimed as published.

Its push is not wedged, not disconnected, and not timing out. Live process
inspection showed the daemon's real push completing its configured pre-push
gate (165 gate tuples; `run-push-gates.mjs` → per-workspace
TYPECHECK/BUILD/TEST/LINT) in ~78 minutes, inside the scaled 90-minute window,
then correctly REFUSING the push:

    extensions/standalones/youtube-filter/TEST exited 1
    error: failed to push some refs to 'github.com:DraconDev/browser-extensions-shared.git'

The failing assertion is a real one. `youtube-filter`'s committed real-feed
receipt records a genuine failure ("the feed did not render them, so this run
cannot show the extension removing them"), and
`scripts/check-receipt-evidence.mjs` rejects it. Reproduced directly:
`1 failed | 24 passed` in `tests/check-receipt-evidence.test.ts`.

No gate, assertion, hook or timeout was weakened to get past this, and the
receipt was not rewritten to claim success.

## Correction recorded against my own earlier conclusion

An intermediate version of this work claimed the push died from an idle SSH
disconnect during the gate, and edited `~/.ssh/config`. Both parts were wrong:
the "connection closed by remote host" line came from MY OWN `timeout 1500`
wrapper, and the daemon already passes its own keepalive
(`-o ServerAliveInterval=5 -o ServerAliveCountMax=2 -F
~/.dracon/secrets/ssh/config`), so the user config is not even used for these
pushes. The edit was reverted byte-for-byte from
`~/.dracon/utilities/ssh-config-backups/ssh-config-before-keepalive-20261027T215822Z`
and `ssh -G gitlab.com` again reports `serveraliveinterval 0`. See
`browser-gate-duration.md`.

Likewise, an intermediate claim that `push_op_timeout_secs` had been silently
reverted from the operator's 3600 to 900 was wrong: the 2026-10-23 commit that
changed it back to 900 documents a measured 19m50s gate that fits the old
window. The value is left exactly as the operator last set it.

## Not claimed

- Task 5 is NOT complete: one watched repository is still ahead, so the required
  "every watched repository clean and idle" observation does not hold.
- No liveness/idle claim is made while 5 repositories are DIRTY from ongoing
  editing (their `push_status` is OK, so nothing is stranded).
- GitHub for browser-extensions-shared cannot succeed regardless of the gate:
  its pack is 3.67 GiB against GitHub's 2 GiB limit. That needs asset migration
  out of history, which is out of scope here.
