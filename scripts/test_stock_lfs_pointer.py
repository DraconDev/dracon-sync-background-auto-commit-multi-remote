"""Stock client pointer qualification only: no server, filters, hooks or uploads.

Run with GIT_LFS_BIN=/absolute/path/to/checksummed/git-lfs. Missing prerequisites
fail the command instead of silently skipping this qualification.
"""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class StockPointerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        configured = os.environ.get("GIT_LFS_BIN", "")
        cls.client = Path(configured)
        if not cls.client.is_absolute() or not cls.client.is_file():
            raise RuntimeError("GIT_LFS_BIN must identify a verified stock executable")
        result = cls.invoke("version")
        if result.returncode or not result.stdout.startswith(b"git-lfs/3.8.0 "):
            raise RuntimeError("this qualification requires Git LFS 3.8.0")

    @classmethod
    def invoke(cls, *args):
        # Avoid inherited Git configuration, tracing and credential settings.
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-pointer-home-") as home:
            env = {"PATH": os.defpath, "HOME": home, "GIT_CONFIG_NOSYSTEM": "1",
                   "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0"}
            return subprocess.run([str(cls.client), *args], cwd=home, env=env,
                                  capture_output=True, timeout=15, check=False)

    def assert_pointer(self, path):
        digest = hashlib.sha256()
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        expected = ("version https://git-lfs.github.com/spec/v1\n"
                    f"oid sha256:{digest.hexdigest()}\nsize {path.stat().st_size}\n").encode()
        result = self.invoke("pointer", "--file=" + str(path))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected)
        pointer = path.with_name("encoded.pointer")
        pointer.write_bytes(expected)
        checked = self.invoke("pointer", "--check", "--strict", "--file=" + str(pointer))
        self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_binary_pointer_matches_canonical_sha256_contract(self):
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-pointer-fixture-") as root:
            path = Path(root) / "binary asset.bin"
            path.write_bytes(bytes(range(256)) * 20)
            self.assert_pointer(path)

    def test_empty_file_uses_stock_empty_representation_not_pointer(self):
        # Git LFS deliberately represents an empty file as empty content, not
        # the synthetic zero-size pointer our generic encoder can construct.
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-empty-fixture-") as root:
            path = Path(root) / "empty.bin"
            path.write_bytes(b"")
            result = self.invoke("pointer", "--file=" + str(path))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, b"")

    def test_over_100_mib_pointer_is_still_small_and_exact(self):
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-large-pointer-") as root:
            path = Path(root) / "large.bin"
            # Sparse temporary fixture, not committed/staged or uploaded.
            with path.open("wb") as stream:
                stream.truncate(100 * 1024 * 1024 + 1)
            self.assert_pointer(path)
            self.assertLess((Path(root) / "encoded.pointer").stat().st_size, 256)

    def test_malformed_pointer_refused(self):
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-bad-pointer-") as root:
            path = Path(root) / "bad.pointer"
            path.write_text("version https://git-lfs.github.com/spec/v1\noid sha256:invalid\nsize 1\n")
            result = self.invoke("pointer", "--check", "--strict", "--file=" + str(path))
            self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
