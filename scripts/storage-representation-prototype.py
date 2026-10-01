#!/usr/bin/env python3
"""Offline representation experiment, isolated to a temporary directory.

Requires git, git-lfs, age, and age-keygen. No live repositories, bucket access,
operator keys, or daemon are used. Not an enrollment/migration command.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    required = ("git", "git-lfs", "age", "age-keygen")
    for tool in required:
        if not shutil.which(tool):
            raise SystemExit(f"Missing prototype dependency: {tool}")
    with tempfile.TemporaryDirectory(prefix="dracon-storage-prototype-") as temporary:
        root = Path(temporary)
        env = dict(os.environ)
        env.update(HOME=str(root / "home"), GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        for key in tuple(env):
            if key.startswith("GIT_") and key not in ("GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL"):
                del env[key]
        (root / "home").mkdir()

        def run(args, cwd=root, *, data=None, check=True):
            result = subprocess.run(args, cwd=cwd, env=env, input=data, capture_output=True)
            if check and result.returncode:
                raise RuntimeError(f"Prototype command failed: {args[0]} {args[1]} (details suppressed)")
            return result

        def init(name):
            repo = root / name
            repo.mkdir()
            run(["git", "-c", "init.templateDir=", "init", "-q"], repo)
            run(["git", "config", "core.hooksPath", os.devnull], repo)
            run(["git", "config", "user.name", "Storage fixture"], repo)
            run(["git", "config", "user.email", "storage@example.invalid"], repo)
            return repo

        key = root / "identity.txt"
        run(["age-keygen", "-o", str(key)])
        recipient = run(["age-keygen", "-y", str(key)]).stdout.decode().strip()
        source = root / "source.bin"
        plaintext = b"private-media-fixture\x00" * 65536
        source.write_bytes(plaintext)
        cipher = root / "encrypted.age"
        run(["age", "-r", recipient, "-o", str(cipher), str(source)])
        encrypted = cipher.read_bytes()
        assert plaintext[:64] not in encrypted
        digest = hashlib.sha256(encrypted).hexdigest()

        lfs = init("lfs")
        run(["git-lfs", "install", "--local", "--skip-smudge", "--skip-repo"], lfs)
        (lfs / ".gitattributes").write_text("payload.age filter=lfs diff=lfs merge=lfs -text\n")
        (lfs / "payload.age").write_bytes(encrypted)
        run(["git", "add", "--", ".gitattributes", "payload.age"], lfs)
        pointer = run(["git", "show", ":payload.age"], lfs).stdout
        expected = f"version https://git-lfs.github.com/spec/v1\noid sha256:{digest}\nsize {len(encrypted)}\n".encode()
        assert pointer == expected
        cached = lfs / ".git/lfs/objects" / digest[:2] / digest[2:4] / digest
        assert cached.read_bytes() == encrypted
        hydrated = root / "lfs-restored.bin"
        run(["age", "-d", "-i", str(key), "-o", str(hydrated), str(cached)])
        assert hydrated.read_bytes() == plaintext
        # Git selects one driver: writing two attributes is not filter composition.
        (lfs / ".gitattributes").write_text("payload.age filter=dracon\npayload.age filter=lfs\n")
        assert run(["git", "check-attr", "filter", "--", "payload.age"], lfs).stdout.endswith(b": lfs\n")

        custom = init("prepared")
        reference = json.dumps({"prototype": 1, "backend": "approved", "sha256": digest, "bytes": len(encrypted), "encryption": "age"}, sort_keys=True).encode() + b"\n"
        prepared = root / "prepared.json"
        prepared.write_text(json.dumps({"source_sha256": hashlib.sha256(plaintext).hexdigest(), "reference": reference.decode()}))
        prepared.chmod(0o600)
        filter_script = root / "clean.py"
        filter_script.write_text('''import hashlib, json, sys
from pathlib import Path
metadata = json.loads(Path(sys.argv[1]).read_text())
digest = hashlib.sha256()
while True:
    chunk = sys.stdin.buffer.read(65536)
    if not chunk:
        break
    digest.update(chunk)
if digest.hexdigest() != metadata["source_sha256"]:
    sys.stderr.write("managed payload has no matching prepared reference\\n")
    sys.exit(1)
sys.stdout.write(metadata["reference"])
''')
        # Git's command string is shell interpreted; use shell quoting for paths.
        import shlex
        clean_command = " ".join(map(shlex.quote, [shutil.which("python3"), str(filter_script), str(prepared)]))
        run(["git", "config", "filter.prepared.clean", clean_command], custom)
        run(["git", "config", "filter.prepared.required", "true"], custom)
        (custom / ".gitattributes").write_text("payload.bin filter=prepared -text\n")
        payload = custom / "payload.bin"
        payload.write_bytes(plaintext)
        run(["git", "add", "--", ".gitattributes", "payload.bin"], custom)
        assert run(["git", "show", ":payload.bin"], custom).stdout == reference
        run(["git", "commit", "-q", "-m", "prototype reference"], custom)
        assert payload.read_bytes() == plaintext
        index_before = (custom / ".git/index").read_bytes()
        payload.write_bytes(plaintext + b"changed")
        assert run(["git", "add", "--", "payload.bin"], custom, check=False).returncode != 0
        assert run(["git", "show", ":payload.bin"], custom).stdout == reference
        assert (custom / ".git/index").read_bytes() == index_before
        cold = root / "cold"
        run(["git", "clone", "-q", "--no-hardlinks", str(custom), str(cold)])
        assert (cold / "payload.bin").read_bytes() == reference
        recovery = root / "recovery.age"
        recovery.write_bytes(encrypted)
        assert hashlib.sha256(recovery.read_bytes()).hexdigest() == digest
        restored = root / "prepared-restored.bin"
        run(["age", "-d", "-i", str(key), "-o", str(restored), str(recovery)])
        assert restored.read_bytes() == plaintext
        print(json.dumps({
            "scope": "isolated offline prototype; no Warden classifier or live provider verification",
            "lfs_ciphertext_pointer_round_trip": True,
            "lfs_existing_history_conversion": "not performed",
            "last_attribute_selects_single_driver": True,
            "prepared_filter_preserves_plaintext_worktree": True,
            "changed_source_is_refused_without_index_mutation": True,
            "cold_clone_contains_portable_reference": True,
            "encrypted_recovery_exact_bytes": True,
            "network_transfers": 0,
            "plaintext_bytes": len(plaintext),
            "ciphertext_bytes": len(encrypted),
        }, indent=2))


if __name__ == "__main__":
    main()
