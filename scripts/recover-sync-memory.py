#!/usr/bin/env python3
"""Request a graceful, PID-safe recycle of an operator-owned sync daemon.

The memory guard keeps its freeze marker until the replacement's footprint
falls. This helper never stops/disables a service, raises a limit or sends KILL.
"""

import argparse
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess


def wait_for_exit(owned, seconds):
    return bool(select.select([owned], [], [], seconds)[0])


def throttled_above_high(cgroup):
    if not cgroup.startswith("/") or ".." in Path(cgroup).parts:
        return False
    root = Path("/sys/fs/cgroup") / cgroup.lstrip("/")
    try:
        high = (root / "memory.high").read_text().strip()
        if high == "max" or int((root / "memory.current").read_text()) <= int(high):
            return False
        full = next(line for line in (root / "memory.pressure").read_text().splitlines()
                    if line.startswith("full "))
        fields = dict(item.split("=", 1) for item in full.split()[1:])
        return float(fields["avg10"]) >= 50
    except (OSError, ValueError, KeyError, StopIteration):
        return False


def readonly_probe(args):
    # Only the exact read-only commands observed pinning this daemon above
    # MemoryHigh. Do not grow this into a process-name reaper or include fetch,
    # index writes, hooks, commit operations or arbitrary Git subcommands.
    return (args[1:] == [b"diff", b"--cached", b"--unified=0"] or
            (args[1:5] == [b"--no-replace-objects", b"rev-list", b"--objects", b"--no-object-names"]
             and len(args) > 5 and all(arg.startswith(b"refs/remotes/") for arg in args[5:])))


def cancel_readonly_probes(parent, git, proc_root=Path("/proc")):
    parent_cgroup = (proc_root / str(parent) / "cgroup").read_bytes()
    cancelled = []
    for proc in proc_root.iterdir():
        if not proc.name.isdigit():
            continue
        owned = None
        try:
            status = dict(line.split(":", 1) for line in (proc / "status").read_text().splitlines() if ":" in line)
            if int(status.get("PPid", "0")) != parent or proc.stat().st_uid != os.getuid():
                continue
            owned = os.pidfd_open(int(proc.name))
            # Recheck after pinning: a PID reuse or a child leaving the unit
            # must not turn this shutdown cancellation into another workload.
            status = dict(line.split(":", 1) for line in (proc / "status").read_text().splitlines() if ":" in line)
            if int(status.get("PPid", "0")) != parent or (proc / "cgroup").read_bytes() != parent_cgroup:
                continue
            if Path(os.readlink(proc / "exe")) != git:
                continue
            args = (proc / "cmdline").read_bytes().rstrip(b"\0").split(b"\0")
            if not readonly_probe(args):
                continue
            signal.pidfd_send_signal(owned, signal.SIGTERM)
            cancelled.append(int(proc.name))
        except (OSError, ValueError):
            # Exited, reparented or unreadable children are never guessed at.
            continue
        finally:
            if owned is not None:
                os.close(owned)
    return cancelled


def request_recovery(unit, executable):
    result = subprocess.run(
        ["systemctl", "show", unit, "-p", "MainPID", "-p", "Restart", "-p", "ControlGroup"],
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
        if not wait_for_exit(owned, 5) and throttled_above_high(properties.get("ControlGroup", "")):
            # A stalled read-only probe can hold enough memory to stop the
            # executor from handling TERM. Cancel only verified probes in that
            # same daemon's cgroup, releasing headroom without raising limits
            # or interrupting repository mutations.
            git = shutil.which("git")
            if git:
                cancelled = cancel_readonly_probes(pid, Path(git).resolve())
                if cancelled:
                    print(f"guard: cancelled read-only shutdown probes via SIGTERM: {cancelled}")
                    wait_for_exit(owned, 5)
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
