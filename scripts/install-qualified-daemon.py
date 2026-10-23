#!/usr/bin/env python3
"""Install the qualified dracon-sync binary into the LIVE operator service.

Matches how this host actually runs the daemon: a system unit
`dracon-sync-operator.service` (Restart=always) executing
`~/.local/bin/dracon-sync daemon`. The legacy `dracon-sync.service` is masked on
this host and is not touched.

Safety properties:
  * the artifact's SHA-256 must match the caller's expectation before anything
    is replaced;
  * the destination must be a regular file (never a symlink);
  * the running process is identified by the unit's MainPID AND verified to be
    `dracon-sync daemon` owned by this user;
  * the current binary and the loaded executable are backed up first;
  * the replacement is atomic (`os.replace` on the same filesystem);
  * SIGTERM is sent only through a pidfd for the verified old PID, so a reused
    PID can never be signalled;
  * the receipt records the NEW pid and the SHA-256 of its loaded `exe`, i.e.
    the actually running revision — not just installation metadata;
  * if the new executable is not observed, the backup is retained and the
    script fails loudly.

Usage: install-qualified-daemon.py <artifact> <expected-sha256> <label>
"""

import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

UNIT = "dracon-sync-operator.service"
ARTIFACT = Path(sys.argv[1]).resolve()
EXPECTED = sys.argv[2]
LABEL = sys.argv[3]
DEST = Path.home() / ".local/bin/dracon-sync"


def digest(path):
    sha = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            sha.update(block)
    return sha.hexdigest()


def loaded_digest(pid, attempts=25):
    """SHA-256 of a running process's executable.

    `/proc/<pid>/exe` can fail transiently while a freshly started process is
    still being set up (or during the moment the old one is reaped), so retry
    briefly instead of mistaking a race for a failed deployment.
    """
    last = None
    for _ in range(attempts):
        try:
            return digest(Path("/proc", str(pid), "exe"))
        except (FileNotFoundError, PermissionError, ProcessLookupError) as error:
            last = error
            time.sleep(0.4)
    raise RuntimeError(f"cannot read /proc/{pid}/exe: {last}")


def main_pid():
    out = subprocess.run(
        ["systemctl", "show", UNIT, "-p", "MainPID", "--value"],
        capture_output=True,
        text=True,
        timeout=10,
    )
    return int(out.stdout.strip() or "0")


def main():
    if digest(ARTIFACT) != EXPECTED:
        raise SystemExit("artifact digest does not match the expected qualified build")
    if not DEST.is_file() or DEST.is_symlink():
        raise SystemExit(f"{DEST} is not a regular file; refusing to replace it")
    old = main_pid()
    if old <= 0:
        raise SystemExit(f"{UNIT} has no MainPID; refusing to install blind")
    if Path("/proc", str(old)).stat().st_uid != os.getuid():
        raise SystemExit("running daemon is not owned by this user")
    cmdline = Path("/proc", str(old), "cmdline").read_bytes()
    if b"dracon-sync\x00daemon" not in cmdline:
        raise SystemExit("running process is not the dracon-sync daemon")

    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    backup = Path.home() / ".local/state/dracon-sync/deployments" / f"{stamp}-{LABEL}"
    backup.mkdir(mode=0o700, parents=True)
    shutil.copy2(DEST, backup / "dracon-sync.before")
    shutil.copyfile(Path("/proc", str(old), "exe"), backup / "dracon-sync.loaded-before")
    receipt = {
        "unit": UNIT,
        "method": "atomic replace + SIGTERM via pidfd; unit Restart=always reloads",
        "label": LABEL,
        "started_at": stamp,
        "old_pid": old,
        "previous_sha256": digest(DEST),
        "expected_sha256": EXPECTED,
        "backup": str(backup),
    }
    owned = os.pidfd_open(old)
    temporary = DEST.with_name(f".dracon-sync.{LABEL}-new")
    try:
        if temporary.exists():
            temporary.unlink()
        shutil.copyfile(ARTIFACT, temporary)
        temporary.chmod(0o755)
        if digest(temporary) != EXPECTED:
            raise RuntimeError("staged replacement digest mismatch")
        os.replace(temporary, DEST)
        if digest(DEST) != EXPECTED:
            raise RuntimeError("installed destination digest mismatch")
        if main_pid() != old:
            raise RuntimeError("daemon changed PID during deployment preparation")
        signal.pidfd_send_signal(owned, signal.SIGTERM)
        deadline = time.monotonic() + 300
        while time.monotonic() < deadline:
            new = main_pid()
            if new > 0 and new != old:
                try:
                    if loaded_digest(new) == EXPECTED:
                        receipt.update(
                            new_pid=new,
                            running_sha256=EXPECTED,
                            finished_at=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                            loaded_revision="verified from /proc/<new-pid>/exe",
                        )
                        (backup / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
                        print(json.dumps(receipt, indent=2))
                        return
                except FileNotFoundError:
                    pass
            time.sleep(1)
        raise RuntimeError(
            "the newly loaded executable was not observed within 300s; backup retained"
        )
    finally:
        os.close(owned)
        if temporary.exists():
            temporary.unlink()


if __name__ == "__main__":
    main()
