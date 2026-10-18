# Next administrator action — v8 (Type=exec readiness fix)

v7 identified the race: post-start sampled root systemd-executor before it
became the operator-owned daemon. Parent matched the reported executable
hash exactly to installed systemd-executor. v8 uses Type=exec so systemctl
waits until execve before the SAME UID1000 and daemon-hash checks run. No
security check relaxed. See migration-v7-executor-race.md. 17 offline tests
pass; unit verification exits0. Parent-inspected change; no new claim of
independent v8 runtime review or successful deployment.

Paste three complete lines in your terminal (include final dot):

```sh
cd ~/.local/state/dracon-sync/service-migration/20261018-operator-v8
p=/nix/store/vm6nxpp97fxgczw00bwkxdqdm2an3n95-python3-3.13.12/bin/python3.13
sudo "$p" -I ./migrate.py apply .
```

Password only at sudo's local prompt. Return the Post-start checks JSON and
result. Use only v8; earlier plans are obsolete or have existing backups.

Frozen plan SHA256: a73bad17c4eddfcae660ac3c6c1ae427e91e8556ec010b28213241189c926c2c

Current original user daemon restored active/enabled. Migration retains
confinement, User=dracon/Group=users, runtime enable, failure observations,
rollback stop confirmation and freeze/hold handling.

Do not reboot mid-qualification. /run units vanish but persistent user mask
remains: restore scheduling or declarative NixOS persistence BEFORE reboot.
After success parent must verify actual service/watchdogs/UID/hash and then
scheduled upload, independent consumer fetch and fresh restoration. A
migration receipt alone does not satisfy the qualification goal.
