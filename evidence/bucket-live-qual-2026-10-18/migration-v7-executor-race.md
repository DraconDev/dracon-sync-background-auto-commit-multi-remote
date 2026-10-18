# v7 root PID observation identified as systemd-executor

Operator v7 report: active MainPID427852, UIDs [0,0,0,0], executable SHA256
712574db3976e5ce11f2a6f93bbe9f849998f45557ebeeea6acf23b18d23a605,
old user unit masked. Verification refused UID mismatch and rolled back.

Parent directly SHA256-hashed /run/current-system/systemd/lib/systemd/systemd-executor
and /run/current-system/sw/lib/systemd/systemd-executor: BOTH exactly match
the observed 712574db... hash. This identifies the captured executable as the
systemd pre-exec launcher, not dracon-sync. The journal subsequently shows
same PID427852 running daemon startup, freeze respected, then clean stop.
Old user daemon is active/enabled MainPID429684; system unit inactive/MainPID0.
No root-daemon authorization or UID/hash-check exception was introduced.

Root cause: inherited Type=simple considers the service started after fork,
before execve/drop-to-User. Immediate post-start /proc sampling races that
transition. v8 explicitly requires the expected original Type=simple and
transforms it to Type=exec. systemctl start now waits for execve and the user
transition; unchanged post-start UID and exact executable hash checks then
verify the daemon, not its launcher. No sleep/retry or ambiguous-identity
acceptance added. Other unit confinement and rollback remain unchanged.

17 offline tests pass (migration-offline-tests-v8.log), including readiness
type conversion and rejection of an unexpected starting readiness type;
unit systemd-analyze verify exit0 (unrelated cups legacy path warning).
No privileged v8 deployment executed. This is readiness verification rather
than proof of application-level bucket operation.

Frozen v8 directory: /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v8
Plan SHA256: a73bad17c4eddfcae660ac3c6c1ae427e91e8556ec010b28213241189c926c2c

Runtime qualification remains ephemeral across reboot. Live upload, consumer
readback and restoration still require execution and evidence.
