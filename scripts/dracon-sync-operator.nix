# Prepared (NOT applied) NixOS module: make the operator-UID dracon-sync
# system service persistent across reboot.
#
# Why: since the 2026-10-19 v8 migration the scheduler runs as a
# system-manager service installed under /run/systemd/system (ephemeral),
# while the user scheduler is masked (persistent). After a reboot no
# scheduler would start. Applying this module makes the SAME reviewed
# unit (frozen plan a73bad17..., Type=exec, User=dracon) declarative.
# The user-unit mask in ~/.config/systemd/user persists on disk and
# stays correct alongside this module.
#
# Operator activation (requires sudo, NOT done by the audit):
#   1. Copy this file into the NixOS config (e.g. ~/.dracon/nixos/modules/)
#      and add it to the host imports.
#   2. sudo nixos-rebuild switch
#   3. Verify: systemctl show dracon-sync-operator.service -p MainPID,Type
#      and `dracon-sync health`.
# Rollback: remove the import and rebuild; or restore user scheduling by
# unmasking the user unit and stopping the system service (see the v8
# handover-backup in ~/.local/state/dracon-sync/service-migration/20261018-operator-v8).
#
# APPLIED 2026-11-11 (memory-collapse incident): installed at
# /home/dracon/nixos/modules/dracon-sync-operator.nix and imported by
# /home/dracon/nixos/configuration.nix, so the operator unit is declarative
# and survives a reboot. The 2026-11-11 change added the containment block
# (MemoryHigh/MemoryMax/MemorySwapMax/IOWeight/OOMPolicy/ManagedOOM*) that
# keeps the sync daemon from taking the desktop down with it; see the
# inline comments on each setting for the measurements behind it.
#
# CHANGED 2026-11-13 (OOM restart loop): daemon pushes no longer run in this
# unit's cgroup. Each `git push` (with its pre-push hook, pack-objects and
# ssh) runs as a transient USER service under `dracon-sync-push.slice`,
# defined below, with this unit's filesystem sandbox mirrored onto it by the
# daemon (src/git/push_unit.rs). Before that, the 1400M cap below covered the
# daemon AND every hook: browser-extensions-shared's gate (measured peak 4G)
# OOM-killed the whole daemon 43 times in 3h, and each restart re-ran the
# same push. The cap below now bounds the daemon itself; the slice bounds
# repository-supplied hook code, and a slice kill costs one push attempt,
# not the fleet's in-flight work.
#
# Stale runtime drop-ins: /run/systemd/system/dracon-sync-operator.service.d/
# from 2026-10-19 OVERRIDE this module until reboot. 10-resource-headroom.conf
# set MemoryHigh=1536M, above MemoryMax=1400M, so soft reclaim never engaged.
# `dracon-sync health` reports that inversion (Containment row). Remove them:
#   sudo rm /run/systemd/system/dracon-sync-operator.service.d/{10-resource-headroom,20-bridge-fixture,30-tasksmax}.conf
#   sudo systemctl daemon-reload
#
# NOTE: the hand-written drop-ins that lived under
# /run/systemd/system/dracon-sync-operator.service.d/ were tmpfs and did not
# survive a reboot. Their contents are folded into this module (TasksMax 512,
# the bridge-fixture ReadWritePaths, the memory headroom), so the unit no
# longer depends on them.

{ lib, ... }:

{
  systemd.services.dracon-sync-operator = {
    description = "Dracon Sync (deterministic sync runtime)";
    documentation = [ "https://github.com/DraconDev/dracon-utilities" ];
    after = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    environment = {
      # lib.mkForce: NixOS's systemd module defines a default PATH for every
      # service, so an unqualified value here is a conflicting definition
      # and the whole config fails to evaluate.
      PATH = lib.mkForce "/home/dracon/.local/bin:/run/wrappers/bin:/home/dracon/.local/share/flatpak/exports/bin:/var/lib/flatpak/exports/bin:/home/dracon/.nix-profile/bin:/nix/profile/bin:/home/dracon/.local/state/nix/profile/bin:/etc/profiles/per-user/dracon/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin";
      DRACON_SYNC_POLICY = "/home/dracon/.dracon/utilities/sync/dracon-sync.toml";
      GIT_TERMINAL_PROMPT = "0";
      HOME = "/home/dracon";
      XDG_RUNTIME_DIR = "/run/user/1000";
      DBUS_SESSION_BUS_ADDRESS = "unix:path=/run/user/1000/bus";
    };
    serviceConfig = {
      Type = "exec";
      User = "dracon";
      Group = "users";
      PrivateUsers = false;
      ExecStartPre = [
        "-/run/current-system/sw/bin/pkill -x -f \"dracon-git pulse\""
        "/etc/profiles/per-user/dracon/bin/node -e \"const s=require('node:fs');if(process.geteuid()!==1000||s.statSync('/').uid!==0||s.statSync('/home').uid!==0)process.exit(78)\""
      ];
      ExecStart = "/home/dracon/.local/bin/dracon-sync daemon";
      Restart = "always";
      # CHANGED 2026-11-11 (memory-collapse incident): 5 -> 30. The unit is
      # killed by systemd-oomd when it becomes the memory hog (see
      # ManagedOOM* below); a 5s restart was a hot loop against a host that
      # was still recovering.
      RestartSec = 30;
      RestartPreventExitStatus = [ 2 78 ];
      Nice = 10;
      CPUQuota = "200%";
      # CHANGED 2026-11-11 (memory-collapse incident): the daemon's memory
      # limits are the ONLY hard containment between the sync daemon and a
      # dead desktop, so they are tightened here and the reasoning is kept:
      #
      #   * MemoryHigh 1536M -> 1024M. The cgroup was pinned at its soft
      #     limit (systemd reported "available: 15.9M" while the daemon held
      #     1.4 GiB), which keeps the kernel in global reclaim -- the daemon
      #     does not merely lose its own pages, it makes the WHOLE host
      #     thrash. Reclaiming earlier and locally is what protects the
      #     desktop. The 2026-09-28 note that raised this from 768M did so
      #     because a 733 MB `git pack-objects` child was being throttled;
      #     1024M still fits that child with room to spare.
      #   * MemoryMax 2G -> 1400M. Unchanged in spirit (it was never the
      #     binding constraint) but now it binds BEFORE the host does: a
      #     runaway child is killed inside the cgroup instead of taking the
      #     desktop with it.
      #   * MemorySwapMax 128M (NEW). This is the important one. The host
      #     runs 60 GiB of zram, so "swap" here is RAM plus zstd CPU. The
      #     daemon's 1.2 GiB of swap (3.4 GiB peak) was invisible in RSS --
      #     which is why the memory guard never named it as an offender --
      #     while it burned CPU on zram decompression and starved the
      #     compositor's page-ins. Keeping the daemon off swap keeps its
      #     footprint visible and stops the amplification. If a legitimate
      #     push needs more, the daemon is OOM-killed and restarted, which
      #     is the intended trade: a restarted sync is cheap, a wedged
      #     desktop is not.
      MemoryHigh = "1024M";
      MemoryMax = "1400M";
      MemorySwapMax = "128M";
      # CHANGED 2026-11-11: IOWeight 10 (default 100). The daemon read
      # 4.6 TB and wrote 1 TB while continuously re-classifying 38 repos;
      # the sync's git traffic competed with the compositor's page-ins on
      # the same NVMe, which is what turned memory pressure into a
      # pageflip timeout in KWin. Low IO weight makes the sync yield to
      # interactive work instead of starving it.
      IOWeight = "10";
      # CHANGED 2026-11-11: hand the kill decision to systemd-oomd, which
      # is already active on this host. When the daemon's swap use crosses
      # its limit (or it becomes a major consumer while the host is under
      # memory pressure), systemd-oomd kills THIS unit and nothing else.
      # The desktop survives; the sync restarts 30s later.
      OOMPolicy = "kill";
      # CHANGED 2026-11-13 (oomd kill loop): ManagedOOMMemoryPressure
      # "kill" -> "auto". systemd-oomd judges memory pressure of THIS
      # cgroup, and that pressure is produced by our own MemoryHigh cap: the
      # daemon sits at 1024M (memory.events high=46992, swap pinned at its
      # 128M limit), stalls in reclaim, crosses oomd's 60%/30s default and
      # is SIGKILLed -- 5 times on 2026-11-13 (15:01, 20:31, 20:52, 21:36,
      # 22:11), each restart re-doing startup work. Killing a unit for the
      # throttling we imposed on it frees nothing. MemoryMax still bounds a
      # genuine overrun (kernel OOM inside the cgroup), and pushes/hooks run
      # in dracon-sync-push.slice with their own oomd policy.
      ManagedOOMMemoryPressure = "auto";
      ManagedOOMSwap = "kill";
      # CHANGED 2026-10-21: 96 -> 512. The shipped unit (dracon-sync.service)
      # and the parent flake's parity check (needSync "TasksMax" "512") already
      # carry 512; this module copy still pinned 96, so a NixOS install of
      # the operator unit would keep the old ceiling. The daemon's own
      # Git filters and Node pre-push workers are the consumers that hit it.
      TasksMax = 512;
      NoNewPrivileges = true;
      ProtectSystem = "strict";
      ProtectHome = "read-only";
      ReadWritePaths = [
        "/home/dracon/.dracon"
        "/home/dracon/Dev"
        "/home/dracon/.local/state/dracon"
        "/home/dracon/.ssh"
        "/home/dracon/dracon/bridge-fixture"
      ];
      PrivateTmp = true;
      PrivateDevices = true;
      ProtectKernelTunables = true;
      ProtectKernelLogs = true;
      ProtectClock = true;
      ProtectHostname = true;
      ProtectControlGroups = true;
      LockPersonality = true;
      RestrictRealtime = true;
      RestrictSUIDSGID = true;
      RemoveIPC = true;
      CapabilityBoundingSet = "";
      RestrictNamespaces = true;
      SystemCallFilter = "@system-service";
      SystemCallErrorNumber = "EPERM";
      StandardOutput = "journal";
      StandardError = "journal";
    };
  };

  # ADDED 2026-11-13: the budget for ALL daemon pushes and their pre-push
  # hooks together. The daemon sets no limits itself; it places each push in
  # this slice (`--slice=dracon-sync-push.slice`). Sized from measurement:
  # browser-extensions-shared's gate held ~2G steady and peaked at 4G under
  # the mirrored sandbox (2026-11-13), and two repositories can push at once.
  #   * MemoryHigh 4G: reclaim starts at the measured single-gate peak.
  #   * MemoryMax 6G: hard stop well below the 30G host; an overrun kills one
  #     push unit (reported as "exhausted its isolated memory budget").
  #   * MemorySwapMax 512M: same reasoning as the daemon's 128M — keep hook
  #     memory visible instead of burning zram CPU — with room for tsc's
  #     cold pages.
  #   * CPU/IO: gates are throughput work. Nice=19 is set per unit by the
  #     daemon; the weights make them yield to the desktop, and the quota
  #     (half of 16 cores) keeps eight parallel typechecks from saturating it.
  systemd.user.slices.dracon-sync-push = {
    description = "dracon-sync daemon pushes and their pre-push hooks";
    sliceConfig = {
      MemoryHigh = "4G";
      MemoryMax = "6G";
      MemorySwapMax = "512M";
      CPUWeight = 20;
      CPUQuota = "800%";
      IOWeight = 10;
      ManagedOOMMemoryPressure = "kill";
    };
  };

  systemd.services.dracon-sync-operator-watchdog = {
    description = "Restart operator-UID sync scheduler if inactive";
    unitConfig.ConditionPathExists = "!/home/dracon/.dracon/dracon-sync.maintenance-hold";
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "/run/current-system/sw/bin/systemctl start dracon-sync-operator.service";
      NoNewPrivileges = true;
      ProtectSystem = "strict";
      ProtectHome = "read-only";
    };
  };

  systemd.timers.dracon-sync-operator-watchdog = {
    description = "Operator sync scheduler watchdog (2 minutes)";
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnBootSec = "2min";
      OnUnitActiveSec = "2min";
      AccuracySec = "10s";
    };
  };
}
