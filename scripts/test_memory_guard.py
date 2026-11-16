#!/usr/bin/env python3
"""Exercise guard recovery using fixture procfs and mock control commands."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch, Mock

SCRIPTS = Path(__file__).resolve().parent


class GuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="sync-memory-guard-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.proc = self.root / "proc"
        (self.proc / "pressure").mkdir(parents=True)
        self.cg = self.root / "cgroup/system.slice/sync.service"
        self.cg.mkdir(parents=True)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.marker = self.root / "freeze"
        self.calls = self.root / "calls"
        self.env = dict(os.environ, HOME=str(self.root), PATH=f"{self.bin}:{os.environ['PATH']}",
                        MEMORY_GUARD_STATE_DIR=str(self.root / "state"),
                        MEMORY_GUARD_MARKER=str(self.marker),
                        MEMORY_GUARD_PROC_ROOT=str(self.proc),
                        MEMORY_GUARD_CGROUP_ROOT=str(self.root / "cgroup"),
                        MEMORY_GUARD_SYNC_BIN=str(self.bin / "sync"),
                        MEMORY_GUARD_RECOVERY_HELPER=str(self.bin / "recover"),
                        MEMORY_GUARD_FREEZE_STREAK="1", MEMORY_GUARD_RECOVER_AFTER_SECS="0")
        self.command("systemctl", "echo /system.slice/sync.service\n")
        self.command("journalctl", "exit 0\n")
        self.command("sync", f'''echo "$1" >> '{self.calls}'
case "$1" in
 pause) echo 'guard freeze' > '{self.marker}' ;;
 resume) test ! -e '{self.root / "resume-fails"}' || exit 1; rm -f '{self.marker}' ;;
esac
''')
        self.command("recover", f"echo recover >> '{self.calls}'\n")
        self.metrics(900)

    def command(self, name, body):
        target = self.bin / name
        target.write_text("#!/bin/sh\n" + body)
        target.chmod(0o700)

    def metrics(self, mb, psi=25, free=100000):
        (self.proc / "meminfo").write_text(
            f"MemTotal: 1000000 kB\nMemFree: {free} kB\nBuffers: 0 kB\nCached: 0 kB\nSReclaimable: 0 kB\n")
        (self.proc / "pressure/memory").write_text(f"full avg10={psi} avg60=0 avg300=0\n")
        (self.proc / "vmstat").write_text("pswpin 100\n")
        (self.cg / "memory.stat").write_text(f"anon {mb * 1024 * 1024}\nslab_unreclaimable 0\nkernel_stack 0\n")

    def tick(self):
        return subprocess.run(["bash", str(SCRIPTS / "dracon-memory-guard.sh")], env=self.env,
                              check=True, capture_output=True, text=True, timeout=5).stdout

    def actions(self):
        return self.calls.read_text().splitlines() if self.calls.exists() else []

    def test_retained_memory_recovers_once_then_low_footprint_resumes_despite_host_psi(self):
        self.tick()
        self.assertTrue(self.marker.exists())
        self.tick()
        self.tick()
        self.assertEqual(self.actions(), ["pause", "recover"])
        self.metrics(100)  # replacement is small, other tenants still stall
        for _ in range(3):
            self.tick()
        self.assertFalse(self.marker.exists())
        self.assertEqual(self.actions(), ["pause", "recover", "resume"])

    def test_manual_pause_is_neither_restarted_nor_resumed(self):
        self.marker.write_text("operator pause")
        for mb in (900, 100, 100, 100):
            self.metrics(mb)
            self.tick()
        self.assertEqual(self.actions(), [])
        self.assertEqual(self.marker.read_text(), "operator pause")

    def test_replaced_owned_pause_is_not_touched_even_while_pressure_persists(self):
        self.tick()
        replacement = self.root / "manual"
        replacement.write_text("manual replacement")
        replacement.replace(self.marker)
        self.tick()
        self.metrics(100)
        for _ in range(3):
            self.tick()
        self.assertEqual(self.actions(), ["pause"])
        self.assertEqual(self.marker.read_text(), "manual replacement")

    def test_resume_failure_keeps_ownership_and_retries(self):
        self.tick()
        self.metrics(100)
        failure = self.root / "resume-fails"
        failure.touch()
        for _ in range(3):
            self.tick()
        self.assertTrue(self.marker.exists())
        failure.unlink()
        self.tick()
        self.assertFalse(self.marker.exists())
        self.assertEqual(self.actions(), ["pause", "resume", "resume"])

    def test_unattributed_host_pressure_does_not_pause_small_daemon(self):
        self.metrics(100)
        for _ in range(3):
            self.tick()
        self.assertEqual(self.actions(), [])

    def test_alternate_manual_marker_blocks_automatic_restart_and_resume(self):
        self.tick()
        alternate = self.root / ".dracon/freeze/dracon-sync"
        alternate.parent.mkdir(parents=True)
        alternate.write_text("independent operator pause")
        self.tick()
        self.metrics(100)
        for _ in range(3):
            self.tick()
        self.assertEqual(self.actions(), ["pause"])
        self.assertTrue(alternate.exists())
        self.assertTrue(self.marker.exists())


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("recovery", SCRIPTS / "recover-sync-memory.py")
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)

    def test_unrestartable_unit_is_never_signalled(self):
        with patch.object(self.module.subprocess, "run", return_value=Mock(stdout="MainPID=123\nRestart=no\n")), \
             patch.object(self.module.os, "pidfd_open") as opened:
            with self.assertRaisesRegex(RuntimeError, "Restart=always"):
                self.module.request_recovery("sync.service", "/installed/dracon-sync")
            opened.assert_not_called()

    def test_verified_daemon_receives_only_term_via_owned_pidfd(self):
        with patch.object(self.module.subprocess, "run", return_value=Mock(stdout="MainPID=123\nRestart=always\n")), \
             patch.object(self.module, "wait_for_exit", return_value=True), \
             patch.object(self.module.os, "pidfd_open", return_value=456), \
             patch.object(self.module.os, "close") as close, \
             patch.object(self.module.Path, "stat", return_value=Mock(st_uid=os.getuid())), \
             patch.object(self.module.Path, "resolve", return_value=Path("/installed/dracon-sync")), \
             patch.object(self.module.os, "readlink", return_value="/installed/dracon-sync"), \
             patch.object(self.module.Path, "read_bytes", return_value=b"/installed/dracon-sync\0daemon\0"), \
             patch.object(self.module.signal, "pidfd_send_signal") as send:
            self.module.request_recovery("sync.service", "/installed/dracon-sync")
            send.assert_called_once_with(456, self.module.signal.SIGTERM)
            close.assert_called_once_with(456)

    def test_probe_cleanup_requires_both_over_high_and_severe_local_pressure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cg = root / "system.slice/sync.service"
            cg.mkdir(parents=True)
            (cg / "memory.high").write_text("1024")
            (cg / "memory.current").write_text("1200")
            (cg / "memory.pressure").write_text("full avg10=68.00 avg60=0 avg300=0\n")
            self.assertTrue(self.module.throttled_above_high("/system.slice/sync.service", root))
            (cg / "memory.pressure").write_text("full avg10=2.00 avg60=0 avg300=0\n")
            self.assertFalse(self.module.throttled_above_high("/system.slice/sync.service", root))
            (cg / "memory.high").write_text("max")
            self.assertFalse(self.module.throttled_above_high("/system.slice/sync.service", root))
            self.assertFalse(self.module.throttled_above_high("/../escape", root))

    def test_shutdown_cancels_only_matching_readonly_git_children_in_the_same_unit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            parent = root / "123"
            parent.mkdir()
            cgroup = "0::/system.slice/sync.service\n"
            (parent / "cgroup").write_text(cgroup)

            def child(pid, args, ppid=123, group=cgroup, binary="/installed/git"):
                proc = root / str(pid)
                proc.mkdir()
                (proc / "status").write_text(f"PPid:\t{ppid}\n")
                (proc / "cgroup").write_text(group)
                (proc / "cmdline").write_bytes(b"\0".join(args) + b"\0")
                (proc / "exe").symlink_to(binary)

            child(1, [b"git", b"diff", b"--cached", b"--unified=0"])
            child(2, [b"git", b"--no-replace-objects", b"rev-list", b"--objects", b"--no-object-names", b"refs/remotes/origin/main"])
            child(3, [b"git", b"fetch", b"origin"])
            child(4, [b"git", b"commit", b"-m", b"work"])
            child(5, [b"git", b"diff", b"--cached", b"--unified=0"], ppid=999)
            child(6, [b"git", b"diff", b"--cached", b"--unified=0"], group="0::/another.service\n")
            child(7, [b"git", b"diff", b"--cached", b"--unified=0"], binary="/repository/fake-git")
            child(8, [b"git", b"update-index", b"--refresh"])
            with patch.object(self.module.os, "pidfd_open", side_effect=lambda pid: pid + 1000), \
                 patch.object(self.module.os, "close"), \
                 patch.object(self.module, "wait_for_exit", return_value=False), \
                 patch.object(self.module.signal, "pidfd_send_signal") as send:
                cancelled = self.module.cancel_readonly_probes(123, Path("/installed/git"), 9999, root)
                self.assertEqual(set(cancelled), {1, 2})
                self.assertEqual({call.args for call in send.call_args_list},
                                 {(1001, self.module.signal.SIGTERM), (1002, self.module.signal.SIGTERM)})
            with patch.object(self.module, "wait_for_exit", return_value=True), \
                 patch.object(self.module.signal, "pidfd_send_signal") as send:
                self.assertEqual(self.module.cancel_readonly_probes(123, Path("/installed/git"), 9999, root), [])
                send.assert_not_called()

    def test_foreign_executable_is_never_signalled_and_fd_is_closed(self):
        with patch.object(self.module.subprocess, "run", return_value=Mock(stdout="MainPID=123\nRestart=always\n")), \
             patch.object(self.module.os, "pidfd_open", return_value=456), \
             patch.object(self.module.os, "close") as close, \
             patch.object(self.module.Path, "stat", return_value=Mock(st_uid=os.getuid())), \
             patch.object(self.module.Path, "resolve", return_value=Path("/installed/dracon-sync")), \
             patch.object(self.module.os, "readlink", return_value="/some/other/process"), \
             patch.object(self.module.signal, "pidfd_send_signal") as send:
            with self.assertRaisesRegex(RuntimeError, "executable"):
                self.module.request_recovery("sync.service", "/installed/dracon-sync")
            send.assert_not_called()
            close.assert_called_once_with(456)


if __name__ == "__main__":
    unittest.main()
