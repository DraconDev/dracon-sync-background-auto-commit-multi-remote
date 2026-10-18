# Ownership probe passed; runtime enable correction (v6)

Operator supplied actual system-manager transient probe result:
`{"euid":1000,"root":0,"home":0}`, exited 0/SUCCESS. Invocation:
2430ee511d3b4f2fa8c0579849387ff6 (operator-transcribed). This proves the tested
system-manager filesystem sandbox retains root ownership visibility. It does
not prove the full daemon starts or uploads.

Parent independently exercised real `systemctl --root=<temporary sysroot>`
without talking to a manager or modifying live paths. A unit installed under
sysroot /run/systemd/system with plain enable produced its wants link under
sysroot /etc/systemd/system. The identical command with --runtime produced
the link under sysroot /run/systemd/system instead. Both exited 0. This is a
concrete script bug and a likely explanation for v3 enable failure against
immutable /etc on this NixOS host; historical stderr was not captured, so the
exact historical cause is not proven.

v6 adds --runtime to BOTH system-manager enable operations (scheduler, timer)
and BOTH matching rollback disable operations. User-manager persistent
enable/disable commands are unchanged. No ownership checks or confinement
removed. Existing failure logging retained.

14 tests pass including real disposable-sysroot enable+disable verifying no
/etc path created, plus fake-manager tripwire rejecting any system enable or
disable lacking --runtime. Simulated receipt in test log is not a deployment
receipt. Log: migration-offline-tests-v6.log.

Frozen v6: /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v6
Script SHA256: fc8369c5e4e4c906e6cf57e3dad15a5d1ed06ea1dbc70d46425e177f93513b8b
Plan SHA256: 3422fa4d50dd8b545fd7abc971157a026f0d374958a07643bd10e35ef90c02fd
Old user daemon verified active. No privileged migration performed by parent.
Do not execute obsolete v2–v5 plans.

Runtime-install reboot limitation remains: /run units disappear but the home
mask remains. Must restore scheduling or install a declarative persistent
service before any reboot; no claim of durable boot persistence is made.
