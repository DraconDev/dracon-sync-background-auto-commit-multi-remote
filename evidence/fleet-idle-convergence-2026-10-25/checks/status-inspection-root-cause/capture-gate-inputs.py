#!/usr/bin/env python3
"""Read-only gate input inventory. Evidence outputs are not product inputs."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[5]
# This script lives in <workspace>/dracon-sync/evidence/<run>/checks/<area>/.
if not (root / 'Cargo.toml').exists():
    root = next(p for p in Path(__file__).resolve().parents
                if (p / 'dracon-sync').is_dir() and (p / 'Cargo.toml').is_file())
record = {'workspace': str(root), 'repositories': {}, 'files': {},
          'excluded': 'evidence/ outputs only; no product code or gate assertion excluded'}
env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
for name in ['.', 'dracon-sync', 'dracon-system', 'dracon-warden']:
    repo = root / name
    def git(*args):
        return subprocess.check_output(['git', '-C', str(repo), *args], env=env, timeout=20)
    record['repositories'][name] = git('rev-parse', 'HEAD').decode().strip()
    for raw in git('ls-files', '-z').split(b'\0'):
        if not raw:
            continue
        rel = os.fsdecode(raw)
        if rel.startswith('evidence/'):
            continue
        path = repo / rel
        key = str(path.relative_to(root))
        if path.is_symlink():
            record['files'][key] = {'symlink': os.readlink(path)}
        elif path.is_file():
            digest = hashlib.sha256()
            with path.open('rb') as handle:
                for chunk in iter(lambda: handle.read(1024 * 1024), b''):
                    digest.update(chunk)
            record['files'][key] = digest.hexdigest()
        else:
            record['files'][key] = {'missing': True}
record['contentSha256'] = hashlib.sha256(json.dumps(record['files'], sort_keys=True).encode()).hexdigest()
Path(sys.argv[1]).write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
print(record['contentSha256'])
