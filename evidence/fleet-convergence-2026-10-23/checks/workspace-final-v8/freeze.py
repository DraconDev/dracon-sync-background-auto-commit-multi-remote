"""Read-only canonical capture with explicit uncommitted compiled-input overlay.

The old runtime cannot commit the repair while its reservations are wedged.
Private refs/checkouts only; no canonical index/ref/config/commit mutation.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

os.umask(0o077)
ROOT = Path('/home/dracon/Dev/dracon-utilities')
OUT = Path(__file__).resolve().parent
STATE = Path(tempfile.mkdtemp(prefix='fleet-worktree-qualified-'))
TREE = STATE / 'source'
manifest = {'started': time.time(), 'state_root': str(STATE), 'frozen_root': str(TREE),
            'target_directory': json.loads((OUT.parent / 'workspace-final-v7/source.json').read_text())['target_directory'], 'repositories': {},
            'compiled_input_files': [], 'overlay': {}, 'status': 'running',
            'source_model': 'captured Git refs plus exact worktree compiled-input overlay; required filters unchanged; private checkouts and target only'}


def save():
    (OUT / 'source.json').write_text(json.dumps(manifest, indent=2) + '\n')


def git(repo, *args):
    result = subprocess.run(['git', '-C', str(repo), *args], capture_output=True, timeout=180)
    if result.returncode:
        raise RuntimeError(f'Git {args[0]} failed ({result.returncode})')
    return result.stdout


def selected(relative, name):
    if relative == '.':
        return name in ('Cargo.toml', 'Cargo.lock', '.cargo/config.toml')
    return name.startswith(('src/', 'tests/', '.cargo/', 'scripts/bucket-runner/')) or name.endswith(('.toml', '.lock', '.rs'))


def inputs(relative):
    source = ROOT / relative
    names = git(source, 'ls-files', '--cached', '--others', '--exclude-standard', '-z').decode().split('\0')
    result = {}
    for name in sorted(set(names)):
        if name and selected(relative, name):
            path = source / name
            if path.is_symlink():
                raise RuntimeError('compiled-input symlink requires explicit closure validation')
            result[name] = path.read_bytes() if path.is_file() else None
    return result


try:
    captures = {relative: inputs(relative) for relative in ('.', 'dracon-sync', 'dracon-system', 'dracon-warden')}
    for relative, captured in captures.items():
        source = ROOT / relative
        ref = git(source, 'rev-parse', 'HEAD').decode().strip()
        algorithm = git(source, 'rev-parse', '--show-object-format').decode().strip()
        objects = git(source, 'rev-parse', '--path-format=absolute', '--git-path', 'objects').decode().strip()
        name = 'parent' if relative == '.' else relative
        bare = STATE / 'bare' / (name + '.git')
        bare.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(['git', 'init', '--quiet', '--bare', '--object-format=' + algorithm, str(bare)], check=True, timeout=30)
        (bare / 'objects/info/alternates').write_text(objects + '\n')
        git(bare, 'update-ref', 'refs/heads/main', ref)
        git(bare, 'symbolic-ref', 'HEAD', 'refs/heads/main')
        destination = TREE if relative == '.' else TREE / relative
        subprocess.run(['git', 'clone', '--shared', '--no-checkout', '--quiet', str(bare), str(destination)], check=True, timeout=180)
        config = subprocess.run(['git', '-C', str(source), 'config', '--local', '--null', '--get-regexp', r'^filter\.dracon\.'], capture_output=True, timeout=30)
        if config.returncode not in (0, 1):
            raise RuntimeError('required filter inspection failed')
        keys = []
        for item in config.stdout.split(b'\0'):
            if item:
                key, value = item.split(b'\n', 1)
                git(destination, 'config', '--local', '--add', key.decode(), value.decode())
                keys.append(key.decode())
        git(destination, 'read-tree', ref)
        git(destination, 'checkout-index', '--all')
        manifest['repositories'][relative] = {'source': str(source), 'ref': ref, 'private_checkout': str(destination), 'copied_existing_filter_keys': keys}
        for name, data in captured.items():
            path = destination / name
            if data is None:
                if path.exists():
                    if not path.is_file():
                        raise RuntimeError('refusing non-file deletion in private assembly')
                    path.unlink()
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            key = str(path.relative_to(TREE))
            manifest['compiled_input_files'].append(key)
            manifest['overlay'][key] = hashlib.sha256(data).hexdigest() if data is not None else None
        save()
    for relative, captured in captures.items():
        if inputs(relative) != captured:
            raise RuntimeError(f'canonical compiled inputs changed during capture: {relative}')
    for base, directories, files in os.walk(TREE, followlinks=False):
        directories[:] = [name for name in directories if name != '.git']
        for name in directories + files:
            path = Path(base) / name
            if path.is_symlink() and not path.resolve().is_relative_to(TREE):
                raise RuntimeError('source link escapes private assembly')
    runner = TREE / 'dracon-sync/scripts/bucket-runner'
    lock = runner / 'package-lock.json'
    manifest['node_dependency_lock_sha256'] = hashlib.sha256(lock.read_bytes()).hexdigest()
    with (OUT / 'node-dependencies.log').open('wb') as log:
        install = subprocess.run(['npm', 'ci', '--no-audit', '--no-fund'], cwd=runner, stdout=log, stderr=subprocess.STDOUT, timeout=300, env={**os.environ, 'GOMAXPROCS': '2'})
    manifest['node_dependency_exit'] = install.returncode
    if install.returncode or hashlib.sha256(lock.read_bytes()).hexdigest() != manifest['node_dependency_lock_sha256']:
        raise RuntimeError('normal locked SDK preparation failed or changed lock')
    manifest['status'] = 'passed'
except BaseException as error:
    manifest['status'] = 'failed'
    manifest['error'] = str(error)
    raise
finally:
    manifest['finished'] = time.time()
    save()
print(json.dumps({'status': manifest['status'], 'frozen_root': str(TREE), 'compiled_inputs': len(manifest['compiled_input_files'])}))
