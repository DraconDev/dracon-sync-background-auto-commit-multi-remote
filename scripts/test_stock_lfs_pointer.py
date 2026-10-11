"""Isolated stock pointer/filter qualification: no server, live hooks or uploads.

Run with GIT_LFS_BIN=/absolute/path/to/checksummed/git-lfs. Missing prerequisites
fail the command instead of silently skipping this qualification.
"""
import hashlib
import os
from pathlib import Path
import subprocess
import shlex
import shutil
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
        return expected

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

    def fixture_env(self, root):
        git = shutil.which("git")
        if not git:
            self.fail("stock filter qualification requires Git")
        return git, {"PATH": str(Path(git).parent) + os.pathsep + os.defpath,
                     "HOME": str(root), "GIT_CONFIG_NOSYSTEM": "1",
                     "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0"}

    def run_fixture(self, args, repo, env, data=None):
        return subprocess.run(args, cwd=repo, env=env, input=data,
                              capture_output=True, timeout=30, check=False)

    def init_fixture(self, root):
        git, env = self.fixture_env(root)
        repo = root / "repo"
        repo.mkdir()
        initialized = self.run_fixture([git, "-c", "init.templateDir=", "init", "-q"], repo, env)
        self.assertEqual(initialized.returncode, 0, initialized.stderr)
        return git, repo, env

    def test_stock_filter_stages_exact_pointer_above_100_mib(self):
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-stage-fixture-") as root:
            git, repo, env = self.init_fixture(Path(root))
            asset = repo / "asset.bin"
            with asset.open("wb") as stream:
                stream.truncate(100 * 1024 * 1024 + 1)
            expected = self.assert_pointer(asset)
            (repo / ".gitattributes").write_text("asset.bin filter=lfs diff=lfs merge=lfs -text\n")
            result = self.run_fixture([git, "-c", "filter.lfs.clean=" + shlex.quote(str(self.client)) + " clean -- %f",
                                       "-c", "filter.lfs.required=true", "add", "--", "asset.bin"], repo, env)
            self.assertEqual(result.returncode, 0, result.stderr)
            staged = self.run_fixture([git, "show", ":asset.bin"], repo, env)
            self.assertEqual(staged.returncode, 0, staged.stderr)
            self.assertEqual(staged.stdout, expected)
            self.assertEqual(asset.stat().st_size, 100 * 1024 * 1024 + 1)
            # This is stock Git staging, NOT a waiver of Sync's size guard.

    def test_required_clean_failure_does_not_stage_raw_bytes(self):
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-filter-refusal-") as root:
            git, repo, env = self.init_fixture(Path(root))
            (repo / "asset.bin").write_bytes(bytes(range(256)))
            (repo / ".gitattributes").write_text("asset.bin filter=lfs -text\n")
            missing = repo / "missing-client"
            result = self.run_fixture([git, "-c", "filter.lfs.clean=" + shlex.quote(str(missing)),
                                       "-c", "filter.lfs.required=true", "add", "--", "asset.bin"], repo, env)
            self.assertNotEqual(result.returncode, 0)
            index = self.run_fixture([git, "ls-files", "--", "asset.bin"], repo, env)
            self.assertEqual(index.returncode, 0, index.stderr)
            self.assertEqual(index.stdout, b"")
            self.assertEqual((repo / "asset.bin").read_bytes(), bytes(range(256)))

    def test_stock_smudge_uses_independent_local_object_copy(self):
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-smudge-fixture-") as root:
            git, producer, env = self.init_fixture(Path(root))
            data = bytes(range(256)) * 20
            cleaned = self.run_fixture([str(self.client), "clean", "--", "asset.bin"], producer, env, data)
            self.assertEqual(cleaned.returncode, 0, cleaned.stderr)
            consumer = Path(root) / "consumer"
            consumer.mkdir()
            initialized = self.run_fixture([git, "-c", "init.templateDir=", "init", "-q"], consumer, env)
            self.assertEqual(initialized.returncode, 0, initialized.stderr)
            # Physically separate directory, no hardlinks/producer cache setting.
            shutil.copytree(producer / ".git/lfs/objects", consumer / ".git/lfs/objects")
            restored = self.run_fixture([str(self.client), "smudge", "--", "asset.bin"], consumer, env, cleaned.stdout)
            self.assertEqual(restored.returncode, 0, restored.stderr)
            self.assertEqual(restored.stdout, data)
            self.assertEqual(hashlib.sha256(restored.stdout).digest(), hashlib.sha256(data).digest())
            # Does not qualify independent server metadata/backup or network fetch.

    def test_malformed_pointer_refused(self):
        with tempfile.TemporaryDirectory(prefix="dracon-lfs-bad-pointer-") as root:
            path = Path(root) / "bad.pointer"
            path.write_text("version https://git-lfs.github.com/spec/v1\noid sha256:invalid\nsize 1\n")
            result = self.invoke("pointer", "--check", "--strict", "--file=" + str(path))
            self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
