#!/usr/bin/env python3
"""Request a graceful, PID-safe recycle of an operator-owned sync daemon.

The memory guard keeps its freeze marker until the replacement's footprint
falls. This helper never stops/disables a service, raises a limit or sends KILL.
"""

import argparse
import os
from pathlib import Path
import signal
import subprocess


def request_recovery(unit, executable):
    result = subprocess.run(
        ["systemctl", "show", unit, "-p", "MainPID", "-p", "Restart"],
        check=True, capture_output=True, text=True, timeout=5,
    )
    properties = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
    if properties.get("Restart") != "always":
        raise RuntimeError("daemon must have Restart=always for graceful memory recovery")
    pid = int(properties.get("MainPID", "0"))
    if pid <= 0:
        raise RuntimeError("daemon has no running MainPID")
    # Pin the target before inspection. If it exits, the descriptor cannot
    # accidentally signal a new process that reused its numeric PID.
    owned = os.pidfd_open(pid)
    try:
        proc = Path("/proc") / str(pid)
        if proc.stat().st_uid != os.getuid():
            raise RuntimeError("daemon is not owned by this user")
        expected = str(Path(executable).resolve())
        loaded = os.readlink(proc / "exe")
        if loaded not in (expected, expected + " (deleted)"):
            raise RuntimeError("daemon executable does not match the installed sync binary")
        args = (proc / "cmdline").read_bytes().split(b"\0")
        if len(args) < 2 or args[1] != b"daemon":
            raise RuntimeError("MainPID is not running the daemon subcommand")
        signal.pidfd_send_signal(owned, signal.SIGTERM)
        print(f"guard: graceful SIGTERM requested for verified sync daemon {pid}")
    finally:
        os.close(owned)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--unit", default="dracon-sync-operator.service")
    parser.add_argument("--executable", required=True)
    args = parser.parse_args()
    try:
        request_recovery(args.unit, args.executable)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        parser.exit(1, f"guard: memory recovery refused: {error}\n")


if __name__ == "__main__":
    main()
