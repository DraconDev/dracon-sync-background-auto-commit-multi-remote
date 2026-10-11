"""Isolated fake-hook regressions; no credentials, real remotes or uploads."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

from lfs_dispatcher_prototype import MAX_REFS_BYTES, Result, dispatch


class DispatcherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dracon-lfs-dispatch-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.hook = self.root / "fake-hook.py"
        self.hook.write_text(
            "import json, os, pathlib, sys, time\n"
            "name, code, delay = sys.argv[1:4]\n"
            "root = pathlib.Path(os.environ['MARKERS'])\n"
            "refs = sys.stdin.buffer.read()\n"
            "(root / (name + '.refs')).write_bytes(refs)\n"
            "(root / (name + '.args')).write_text(json.dumps(sys.argv[4:]))\n"
            "with (root / 'order').open('a') as f: f.write(name + '\\n')\n"
            "time.sleep(float(delay))\n"
            "sys.exit(int(code))\n"
        )
        self.env = dict(os.environ, MARKERS=str(self.root))
        self.env.pop("DRACON_LFS_DISPATCH_ACTIVE", None)
        self.refs = (b"refs/heads/main " + b"a" * 40 + b" refs/heads/main " + b"0" * 40 + b"\n"
                     + b"refs/tags/v1 " + b"b" * 40 + b" refs/tags/v1 " + b"0" * 40 + b"\n")
        self.args = ("origin", "https://example.invalid/repo.git")

    def command(self, name, code=0, delay=0):
        return (str(Path(sys.executable).resolve()), str(self.hook), name, str(code), str(delay))

    def run_dispatch(self, guards=None, transfers=None, **kwargs):
        return dispatch(guards=guards if guards is not None else [self.command("guard")],
                        transfers=transfers if transfers is not None else [self.command("upload")],
                        push_args=self.args, refs=kwargs.pop("refs", self.refs),
                        env=kwargs.pop("env", self.env), **kwargs)

    def order(self):
        marker = self.root / "order"
        return marker.read_text().splitlines() if marker.exists() else []

    def test_security_before_every_transfer_and_exact_replay(self):
        result = self.run_dispatch(guards=[self.command("warden"), self.command("payload")],
                                   transfers=[self.command("foreign"), self.command("local")])
        self.assertEqual(result, Result("ready-for-git", 0))
        self.assertEqual(self.order(), ["warden", "payload", "foreign", "local"])
        for name in self.order():
            self.assertEqual((self.root / (name + ".refs")).read_bytes(), self.refs)
            self.assertEqual(json.loads((self.root / (name + ".args")).read_text()), list(self.args))

    def test_each_guard_refusal_prevents_all_upload_hooks(self):
        for failed in [0, 1]:
            with self.subTest(failed=failed):
                guards = [self.command("g0", 7 if failed == 0 else 0),
                          self.command("g1", 7 if failed == 1 else 0)]
                self.assertEqual(self.run_dispatch(guards=guards), Result("security", 7))
                self.assertNotIn("upload", self.order())

    def test_transfer_failure_stops_remaining_transfers_without_fallback(self):
        self.assertEqual(self.run_dispatch(transfers=[self.command("first", 9), self.command("second")]),
                         Result("transfer", 9))
        self.assertEqual(self.order(), ["guard", "first"])

    def test_missing_required_executable_fails_before_any_hook(self):
        self.assertEqual(self.run_dispatch(transfers=[(str(self.root / "missing"),)]),
                         Result("configuration", 1))
        self.assertEqual(self.order(), [])

    def test_missing_guard_and_recursion_fail_closed(self):
        self.assertEqual(self.run_dispatch(guards=[]), Result("configuration", 1))
        self.assertEqual(self.run_dispatch(env=dict(self.env, DRACON_LFS_DISPATCH_ACTIVE="1")),
                         Result("recursion", 1))
        self.assertEqual(self.order(), [])

    def test_malformed_and_oversized_ref_input_never_invokes_hooks(self):
        for refs in [b"bad\n", self.refs[:-1], b"x" * (MAX_REFS_BYTES + 1), self.refs + b"\x00"]:
            self.assertEqual(self.run_dispatch(refs=refs), Result("configuration", 1))
        self.assertEqual(self.order(), [])

    def test_empty_refs_deletion_and_sha256_replay(self):
        for refs in [b"", b"(delete) " + b"0" * 40 + b" refs/heads/old " + b"a" * 40 + b"\n",
                     b"refs/heads/main " + b"b" * 64 + b" refs/heads/main " + b"0" * 64 + b"\n"]:
            self.assertEqual(self.run_dispatch(refs=refs), Result("ready-for-git", 0))
            self.assertEqual((self.root / "upload.refs").read_bytes(), refs)
        # Acceptance of syntax is not permission to delete: real guards decide.

    def test_timeout_in_security_prevents_upload(self):
        self.assertEqual(self.run_dispatch(guards=[self.command("slow", delay=5)], timeout_seconds=0.2),
                         Result("security", 124))
        self.assertNotIn("upload", self.order())

    def test_transfer_timeout_has_separate_attribution(self):
        self.assertEqual(self.run_dispatch(transfers=[self.command("slow", delay=5)], timeout_seconds=0.2),
                         Result("transfer", 124))
        self.assertEqual(self.order(), ["guard", "slow"])


if __name__ == "__main__":
    unittest.main()
