"""Disposable loopback Gitea API qualification, not OVH/live deployment.

Requires a verified Gitea 28.1.0 executable via GITEA_BIN. All repositories,
credentials, objects and metadata are synthetic and kept outside Git. No
producer/consumer Git commits, live hooks, external requests or stock-client
upload qualification are performed by this test.
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import pwd
import secrets
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request


class GiteaLfsApiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        binary = Path(os.environ.get("GITEA_BIN", ""))
        if not binary.is_absolute() or not binary.is_file():
            raise RuntimeError("GITEA_BIN must identify a verified Gitea executable")
        cls.temp = tempfile.TemporaryDirectory(prefix="dracon-gitea-api-fixture-")
        cls.addClassCleanup(cls.temp.cleanup)
        cls.root = Path(cls.temp.name)
        git = shutil.which("git")
        if not git:
            raise RuntimeError("Git is required")
        cls.env = {"PATH": str(Path(git).parent) + os.pathsep + os.defpath,
                   "HOME": str(cls.root), "GIT_CONFIG_NOSYSTEM": "1",
                   "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0",
                   "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "init.templateDir",
                   "GIT_CONFIG_VALUE_0": ""}
        version = subprocess.run([str(binary), "--version"], env=cls.env,
                                 capture_output=True, timeout=15, check=True)
        if not version.stdout.startswith(b"gitea version 28.1.0 "):
            raise RuntimeError("qualification requires Gitea 28.1.0")
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        # A random URL prefix prevents touching another service in a port race.
        cls.base = f"http://127.0.0.1:{port}/{secrets.token_hex(12)}"
        config = cls.root / "app.ini"
        config.write_text(f"""APP_NAME = Dracon isolated LFS qualification
RUN_USER = {pwd.getpwuid(os.getuid()).pw_name}
RUN_MODE = prod
[server]
HTTP_ADDR = 127.0.0.1
HTTP_PORT = {port}
ROOT_URL = {cls.base}/
APP_DATA_PATH = {cls.root}/data
DISABLE_SSH = true
LFS_START_SERVER = true
[database]
DB_TYPE = sqlite3
PATH = {cls.root}/gitea.db
[repository]
ROOT = {cls.root}/repositories
[lfs]
STORAGE_TYPE = local
PATH = {cls.root}/lfs
[security]
INSTALL_LOCK = true
SECRET_KEY = {secrets.token_hex(32)}
[service]
DISABLE_REGISTRATION = true
REQUIRE_SIGNIN_VIEW = true
[cron]
ENABLED = false
[log]
MODE = console
LEVEL = Error
""")
        config.chmod(0o600)
        cls.command = [str(binary), "--work-path", str(cls.root), "--config", str(config)]
        cls.cli("migrate")
        # Generated password output is captured, never shown or stored in Git.
        cls.cli("admin", "user", "create", "--username", "fixture",
                "--email", "fixture@example.invalid", "--random-password",
                "--must-change-password=false")
        token_output = cls.cli("admin", "user", "generate-access-token", "--username", "fixture",
                               "--token-name", "qualification", "--scopes", "all", "--raw")
        cls.token = token_output.decode().strip()
        if len(cls.token) != 40 or any(c not in "0123456789abcdef" for c in cls.token):
            raise RuntimeError("unexpected token output (redacted)")
        cls.log = (cls.root / "server.log").open("wb")
        cls.addClassCleanup(cls.log.close)
        cls.server = subprocess.Popen([*cls.command, "web"], env=cls.env,
                                      stdout=cls.log, stderr=subprocess.STDOUT, start_new_session=True)
        cls.addClassCleanup(cls.stop_server)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if cls.server.poll() is not None:
                raise RuntimeError("isolated server exited; local log is not published")
            try:
                status, _ = cls.request("/api/v1/version")
                if status == 200:
                    break
            except (OSError, urllib.error.URLError):
                pass
            time.sleep(0.1)
        else:
            raise RuntimeError("isolated server readiness deadline expired")
        for name in ["private", "other"]:
            status, _ = cls.request("/api/v1/user/repos", {"name": name, "private": True}, auth=True)
            if status != 201:
                raise RuntimeError(f"fixture repository creation refused: HTTP {status}")

    @classmethod
    def cli(cls, *args):
        result = subprocess.run([*cls.command, *args], env=cls.env,
                                capture_output=True, timeout=30, check=False)
        if result.returncode:
            raise RuntimeError("isolated Gitea CLI failed (output redacted)")
        return result.stdout

    @classmethod
    def stop_server(cls):
        try:
            os.killpg(cls.server.pid, signal.SIGTERM)
            cls.server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(cls.server.pid, signal.SIGKILL)
            cls.server.wait(timeout=5)
        except ProcessLookupError:
            cls.server.wait(timeout=5)

    @classmethod
    def request(cls, path, payload=None, auth=False, method=None):
        url = path if path.startswith("http:") else cls.base + path
        # Never follow server-provided object URLs outside this owned endpoint.
        if not url.startswith(cls.base + "/"):
            raise RuntimeError("object URL escaped isolated endpoint")
        headers = {"Content-Type": "application/vnd.git-lfs+json"}
        data = None
        if payload is not None:
            data = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        if auth:
            headers["Authorization"] = "Basic " + base64.b64encode(("fixture:" + cls.token).encode()).decode()
        if path.startswith("/api/v1/"):
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        # Redirects could forward auth beyond loopback; refuse them entirely.
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, *args, **kwargs):
                return None
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
        try:
            with opener.open(req, timeout=5) as response:
                return response.status, response.read(1024 * 1024)
        except urllib.error.HTTPError as error:
            return error.code, error.read(1024 * 1024)

    def test_authenticated_upload_download_and_anonymous_refusal(self):
        data = b"non-secret synthetic qualification payload\x00" * 100
        oid = hashlib.sha256(data).hexdigest()
        payload = {"operation": "upload", "transfers": ["basic"],
                   "objects": [{"oid": oid, "size": len(data)}]}
        endpoint = "/fixture/private.git/info/lfs/objects/batch"
        status, _ = self.request(endpoint, payload)
        self.assertIn(status, [401, 403, 404])
        status, response = self.request(endpoint, payload, auth=True)
        self.assertEqual(status, 200)
        upload = json.loads(response)["objects"][0]["actions"]["upload"]["href"]
        status, _ = self.request(upload, data, auth=True, method="PUT")
        self.assertIn(status, [200, 201, 204])
        payload["operation"] = "download"
        status, response = self.request(endpoint, payload, auth=True)
        self.assertEqual(status, 200)
        download = json.loads(response)["objects"][0]["actions"]["download"]["href"]
        status, restored = self.request(download, auth=True)
        self.assertEqual(status, 200)
        self.assertEqual(restored, data)
        self.assertEqual(hashlib.sha256(restored).hexdigest(), oid)
        status, _ = self.request(download)
        self.assertIn(status, [401, 403, 404])
        # A known OID is not access to an unassociated private repository.
        status, response = self.request("/fixture/other.git/info/lfs/objects/batch", payload, auth=True)
        self.assertEqual(status, 200)
        obj = json.loads(response)["objects"][0]
        self.assertIn("error", obj)
        self.assertNotIn("download", obj.get("actions", {}))


if __name__ == "__main__":
    unittest.main()
