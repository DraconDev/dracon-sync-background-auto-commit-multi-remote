# Verification: HTTPS carries the same push that SSH cannot

## The comparative experiment

Same repository, same commit, same real pre-push hook (full 4-gate), only the
transport differs.

### SSH to gitlab.com (what the daemon used)

    [630s] EXITED by server
    remote: ERROR: running upload-pack: waiting for negotiation: context canceled
    Transferred: sent 7640, received 5600 bytes, in 600.4 seconds
    debug1: Exit status 1

The server closed the session at 600.4s. Keepalives were demonstrably flowing
(`client_input_global_request: rtype keepalive@openssh.com want_reply 1`, 11+
exchanges), so this is a SERVER-SIDE lifetime cap, not an idle timeout that
keepalives could rescue.

### HTTPS to gitlab.com, WITH the real pre-push hook

    🔍 Pre-push: 1 affected workspace(s) at b17af4d7f2c9 — running 4-gate.
      ▸ extensions/standalones/web-time-tracker-limiter TYPECHECK
      ▸ extensions/standalones/web-time-tracker-limiter BUILD
      ▸ extensions/standalones/web-time-tracker-limiter TEST
      ▸ extensions/standalones/web-time-tracker-limiter LINT
    ✅ Pre-push: 4-gate passed for all affected workspaces.
    To https://gitlab.com/DraconDev/browser-extensions-shared.git
       2a1bdcf12c..b17af4d7f2  HEAD -> main
    rc=0 at 11:34:33

This ran for ~9 minutes with the gate executing in full and completed
successfully. There is no long-lived SSH session to be capped, and no hook was
skipped (`--dry-run` only suppresses the final ref write; the hook runs
identically, and its verdict line above is real).

## What the code change fixes

`is_local_hook_rejection` now requires positive REFUSAL evidence:

- `gate_refusal_markers()` — `4-gate failed`, `possible plaintext secrets`,
  `dracon-warden:`, `refusing`, `blocked this push`, `pre-commit`, …
- `gate_passed_markers()` — `4-gate passed for all`, `4-gate passed` — returns
  NOT-a-refusal.

Deliberately NOT treated as a local refusal marker: `hook declined`, which is a
SERVER-side string (`pre-receive hook declined`) and must stay in the
permanent-rejection arm so the two sets remain disjoint.

A bare banner with no verdict at all still classifies as a refusal: a gate that
cannot report its outcome must not be retried blindly.

## Regression tests

`test_passing_gate_banner_is_not_a_local_hook_refusal` uses the VERBATIM
incident shape (success verdict + transport closure in one error text) and also
asserts the four adjacent behaviours that must not regress:

- banner-only (no verdict) is still a refusal;
- `4-gate FAILED` is still a local refusal;
- a pass banner coexisting with `pre-receive hook declined` stays
  server-side policy;
- the passing shape is no longer reported as `local pre-push hook`.

Full push test module: 35 passed, 0 failed.
Full `dracon-sync` binary suite: 1519 passed, 0 failed, 3 ignored.
`cargo clippy -p dracon-sync --all-targets -- -D warnings`: clean.
