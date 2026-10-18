#!/usr/bin/env python3
"""One-shot console-verified endpoint correction; run through sync maintenance.
No credentials, namespaces, IAM, bindings or source assets are modified.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
from datetime import datetime, timezone

home = Path.home()
profile_path = home / '.dracon/state/bucket-runner/profile.json'
config_path = home / '.dracon/utilities/sync/dracon-sync.toml'
fixture = str(home / 'dracon/bridge-fixture')
paths = [profile_path, config_path]
originals = {}
for path in paths:
    assert path.is_file() and not path.is_symlink() and path.stat().st_uid == os.getuid()
    originals[path] = path.read_bytes()
profile = json.loads(originals[profile_path])
assert profile['storage']['endpoint'] == 'https://s3.sbg.perf.cloud.ovh.net'
assert profile['storage']['region'] == 'sbg'
config = originals[config_path].decode()
roots = tomllib.loads(config)['watch_roots']
assert fixture not in roots
old = 'watch_roots = [\n    "/home/dracon/.dracon",\n    "/home/dracon/Dev"\n]'
assert config.count(old) == 1
new = 'watch_roots = [\n    "/home/dracon/.dracon",\n    "/home/dracon/Dev",\n    "/home/dracon/dracon/bridge-fixture"\n]'
config = config.replace(old, new)
assert tomllib.loads(config)['watch_roots'] == roots + [fixture]
profile['storage']['endpoint'] = 'https://s3.uk.io.cloud.ovh.net'
profile['storage']['region'] = 'uk'
updated = {profile_path: (json.dumps(profile, indent=2) + '\n').encode(), config_path: config.encode()}
backup = home / '.local/state/dracon-sync/endpoint-correction' / datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
backup.mkdir(parents=True, mode=0o700)
for i, path in enumerate(paths):
    shutil.copy2(path, backup / str(i))
(backup / 'paths.json').write_text(json.dumps([str(p) for p in paths]))

def atomic(path, data, mode):
    fd, name = tempfile.mkstemp(prefix='.endpoint-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as f:
            os.fchmod(f.fileno(), mode)
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name): os.unlink(name)

try:
    for path in paths:
        assert path.read_bytes() == originals[path], 'concurrent operator edit'
        atomic(path, updated[path], path.stat().st_mode & 0o777)
    subprocess.run(['systemctl', '--user', 'restart', 'dracon-sync.service'], check=True, timeout=90)
    subprocess.run(['systemctl', '--user', 'is-active', 'dracon-sync.service'], check=True, timeout=15)
except BaseException:
    for path in paths:
        if path.read_bytes() == updated[path]:
            atomic(path, originals[path], path.stat().st_mode & 0o777)
    subprocess.run(['systemctl', '--user', 'restart', 'dracon-sync.service'], timeout=90)
    raise
print(json.dumps({'at':datetime.now(timezone.utc).isoformat(), 'backup':str(backup),
                  'endpoint':profile['storage']['endpoint'], 'region':'uk', 'addedWatchRoot':fixture,
                  'sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}))
