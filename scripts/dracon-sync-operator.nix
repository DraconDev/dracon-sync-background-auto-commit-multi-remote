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

{ ... }:

{
  systemd.services.dracon-sync-operator = {
    description = "Dracon Sync (deterministic sync runtime)";
    documentation = [ "https://github.com/DraconDev/dracon-utilities" ];
    after = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    environment = {
      PATH = "/home/dracon/.local/bin:/run/wrappers/bin:/home/dracon/.local/share/flatpak/exports/bin:/var/lib/flatpak/exports/bin:/home/dracon/.nix-profile/bin:/nix/profile/bin:/home/dracon/.local/state/nix/profile/bin:/etc/profiles/per-user/dracon/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin";
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
      RestartSec = 5;
      RestartPreventExitStatus = [ 2 78 ];
      Nice = 10;
      CPUQuota = "200%";
      MemoryHigh = "1536M";
      MemoryMax = "2G";
      TasksMax = 96;
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
