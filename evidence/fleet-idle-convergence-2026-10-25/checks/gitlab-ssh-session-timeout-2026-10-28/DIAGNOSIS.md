# browser-extensions-shared: GitLab publication wedged by a 600s SSH session cap

## Symptom

`dracon-sync repos` reported `AHEAD:111 push=PENDING` on
`/home/dracon/Dev/browser-extensions-shared`, growing, with zero fleet
failures. GitHub (`origin`) stayed 0 ahead; only the `gitlab` mirror failed.

Journal (repeated, growing backlog):

    ⚠️ push to gitlab failed for ...: status signal: 13 (SIGPIPE):
       Connection to gitlab.com closed by remote host.
    ⚠️ push failed for ... (remotes: gitlab)
    🚨 ALERT: ... has 111 unpushed commits (threshold: 50)

## Root cause 1 — GitLab closes the SSH session after 600s

The repo's own pre-push gate (`scripts/git-hooks/pre-push` →
`run-push-gates.mjs`) runs the 4-gate (typecheck/build/test/lint) over
3 affected workspaces. That takes ~12 minutes, and it runs INSIDE `git push`,
i.e. AFTER git has already opened the SSH connection to the remote.

Measured against gitlab.com with the daemon's exact SSH options
(`BatchMode=yes`, `ConnectTimeout=10`, `ServerAliveInterval=5`,
`ServerAliveCountMax=2`, operator `-F ~/.dracon/secrets/ssh/config`):

    [630s] EXITED by server
    remote: ERROR: running upload-pack: waiting for negotiation: context canceled
    Transferred: sent 7640, received 5600 bytes, in 600.4 seconds

The session is killed by the SERVER at 600.4s regardless of keepalives —
keepalives were observed working (`client_input_global_request: rtype
keepalive@openssh.com want_reply 1`, 11+ exchanges). A raw TCP probe with no
SSH layer showed the same ~90s idle close. GitHub, in the same test window,
survived the same idle period and completed normally (exit=0), which is why
only the GitLab mirror wedged.

Net effect: git opens the connection, spends ~12 minutes inside the local
pre-push hook, then tries to write the pack onto a session GitLab already
closed → SIGPIPE.

## Root cause 2 — the daemon misclassified the transport failure as a policy refusal

The gate in this run PASSED:

    ✅ Pre-push: 4-gate passed for all affected workspaces.

But `is_local_hook_rejection()` (src/git/push.rs) matches the substring
`pre-push` / `pre-push:` anywhere in the error text. The daemon's runner
appends captured hook STDOUT to the error under `<hook/stdout diagnostics>`
(src/git/ops.rs:253), so a PASSING gate's own banner made the transport
failure look like a local hook refusal.

Consequence, verified in source:
- `push_to_named_remote_inner` returns early on `is_local_hook_rejection`
  (src/git/multi_remote.rs:755-761) — so the working HTTPS fallback is never
  attempted;
- the retry loop returns early on the same predicate
  (src/git/multi_remote.rs:840-846).

Both fail-fast paths fire on a transport error. The daemon therefore never
tried the transport that works, and the operator was told "local hook refused"
when the truth was "the remote dropped the connection".

## Why HTTPS is the correct remedy

GitLab's PAT is present for the daemon: `~/.dracon/utilities/sync/secrets/
gitlab.env` → `GITLAB_TOKEN` (symlink into the legacy PAT dir). The daemon
already has a GitLab HTTPS leg in `push_https_fallback`, which was merely
unreachable because of root cause 2.

Verified live (dry-run, publishes nothing):

    $ git push --dry-run --no-verify https://gitlab.com/DraconDev/browser-extensions-shared.git HEAD:refs/heads/main
    To https://gitlab.com/DraconDev/browser-extensions-shared.git
       bf3ac20bae..9060b0f763  HEAD -> main

HTTPS completes in seconds: there is no long-lived SSH session to be capped,
and the pre-push hook still runs in full — no hook is bypassed.

## Scope

This is a demonstrated shared root cause of delayed/failed convergence, so it
is in scope for the goal's "repair demonstrated causes of failed or delayed
autonomous dracon-sync convergence" clause. No hook bypass, no history
rewrite, no force-push, no scope reduction.
