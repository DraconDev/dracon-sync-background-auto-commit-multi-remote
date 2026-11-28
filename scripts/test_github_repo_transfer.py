import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "github_transfer", Path(__file__).with_name("transfer-large-github-repos.py")
)
transfer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transfer)


class GithubTransferTest(unittest.TestCase):
    def setUp(self):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        self.repo = Path(work.name) / "repo"
        self.repo.mkdir()
        subprocess.run(["git", "init", "-q", self.repo], check=True)
        subprocess.run(["git", "-C", self.repo, "remote", "add", "origin",
                        "git@github.com:personal/example.git"], check=True)
        self.options = SimpleNamespace(
            source="personal", destination="team", state=Path(work.name), apply=True
        )
        self.sha = "a" * 40
        self.owner = "personal"
        self.posts = 0

    def api(self, endpoint, *args, absent=False):
        if endpoint == "repos/personal/example/transfer":
            self.posts += 1
            return {}
        if endpoint == "repos/team/example" and self.owner == "personal":
            return None
        if "/git/ref/heads/main" in endpoint:
            return {"object": {"sha": self.sha}}
        return {
            "id": 123, "full_name": f"{self.owner}/example", "visibility": "private",
            "default_branch": "main", "disabled": False,
            "owner": {"type": "User" if self.owner == "personal" else "Organization"},
            "permissions": {"admin": True},
        }

    def test_reconciles_submitted_transfer_by_id(self):
        actual_git = transfer.git

        def git(repo, *args, **kwargs):
            if args[0] == "ls-remote":
                return self.sha + "\trefs/heads/main"
            return actual_git(repo, *args, **kwargs)

        with (patch.object(transfer, "api", side_effect=self.api),
              patch.object(transfer, "git", side_effect=git),
              patch.object(transfer, "own_bytes", return_value=transfer.LIMIT_BYTES + 1)):
            with self.assertRaises(transfer.Busy):
                transfer.transfer(self.repo, "personal/example", self.options)
            journal = self.options.state / "123.json"
            self.assertEqual(json.loads(journal.read_text())["sha"], self.sha)
            with self.assertRaises(transfer.Busy):
                transfer.transfer(self.repo, "personal/example", self.options, journal)
            self.owner = "team"
            transfer.transfer(self.repo, "personal/example", self.options, journal)

        self.assertEqual(self.posts, 1)
        self.assertFalse(journal.exists())
        self.assertEqual(actual_git(self.repo, "remote", "get-url", "origin"),
                         "git@github.com:team/example.git")
        relative = ".dracon/dracon-sync.toml"
        self.assertEqual(actual_git(self.repo, "check-ignore", relative), relative)
        self.assertIn('exclude_remotes = ["github"]', (self.repo / relative).read_text())

    def test_exactly_two_gib_does_not_transfer(self):
        with (patch.object(transfer, "api") as api,
              patch.object(transfer, "own_bytes", return_value=transfer.LIMIT_BYTES)):
            transfer.transfer(self.repo, "personal/example", self.options)
        api.assert_not_called()


if __name__ == "__main__":
    unittest.main()
