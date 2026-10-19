# browser-extensions-shared push failure — cause and durable repair

## What was observed

11:01:33 daemon: "background push to origin failed for
/home/dracon/Dev/browser-extensions-shared: git push failed ... exit status:
1: error: failed to push some refs to 'github.com:DraconDev/browser-extensions-shared.git'"
and 11:03:28 "push failed for ... (remotes: origin)". The daemon discarded
Git's STDOUT, so the pre-push hook's own verdict (it prints the failing
workspace gate and the /tmp log path) was never retained. The daemon
therefore could not distinguish a local 4-gate refusal from a transport
failure.

## Root cause of the DIAGNOSIS gap (not the original refusal)

The pre-push hook runs each affected workspace's typecheck/build/test/lint
and prints the failing gate to STDOUT; Git relays hook output on stdout.
`git push` was spawned with stdout = /dev/null, so a hook refusal looked
identical to a network error. The hook's own classification in
dracon-sync/src/git/push.rs already recognizes "4-gate failed" /
"pre-push:" in the error text, but the text never reached it.

## Durable repair

`git push` children now capture stdout (bounded 64 KiB prefix, always
drained to EOF so a full pipe cannot deadlock the push), and the captured
hook/stdout diagnostics are appended to the failure error after credential
redaction. Non-push commands keep stdout = /dev/null. Regression test
`failed_hook_stdout_is_bounded_drained_and_retained` proves a failing hook's
stdout survives into the error, the stderr is retained, and the diagnostic
is bounded. The pre-existing classification can now recognize the gate
refusal class instead of misreporting it as a transport failure.

## State of the original push failure

The push recovered on its own at 11:17:48 and the repository has pushed
continuously since (12:23:38 commit 141364a6f502 is on origin+gitlab). The
workspace 4-gate passes today (tabmon: 42 test files / 708 tests pass;
typecheck, build and lint pass — /tmp/pre-push-extensions_standalones_tabmon-*.log).
The original refusal's exact gate is NOT recoverable: the hook's stdout was
discarded and the per-gate /tmp logs were overwritten by later pushes.
This report therefore does not claim the original cause; it fixes the
recurrence of an undiagnosable push failure.
