"""Bounded required gates for the captured worktree assembly, never live source."""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

OUT = Path(__file__).resolve().parent
SOURCE = json.loads((OUT / 'source.json').read_text())
assert SOURCE['status'] == 'passed'
ROOT = Path(SOURCE['frozen_root'])
ENV = {**os.environ, 'CARGO_BUILD_JOBS': '2', 'RUST_TEST_THREADS': '1',
       'CARGO_TARGET_DIR': SOURCE['target_directory']}


def snapshot():
    result = {}
    for name in SOURCE['compiled_input_files']:
        path = ROOT / name
        result[name] = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    return result


manifest = {'started': time.time(), 'qualifier_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'commands': [], 'status': 'running', 'before': snapshot(),
            'environment': {key: ENV[key] for key in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'CARGO_TARGET_DIR')},
            'source_model': SOURCE['source_model']}
assert manifest['before'] == SOURCE['overlay'], 'private compiled inputs differ from actual capture'
active = None


def save():
    (OUT / 'receipt.json').write_text(json.dumps(manifest, indent=2) + '\n')


def cancel(number, frame):
    if active is not None and active.poll() is None:
        try:
            os.killpg(active.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        active.wait(timeout=10)
    manifest['status'] = 'cancelled'
    save()
    raise SystemExit(128 + number)


signal.signal(signal.SIGTERM, cancel)
signal.signal(signal.SIGINT, cancel)
save()
try:
    for name, command, deadline in [
        ('workspace', ['cargo', 'test', '--workspace', '--locked'], 1800),
        ('clippy', ['cargo', 'clippy', '--workspace', '--locked', '--all-targets', '--', '-D', 'warnings'], 1200),
        ('release', ['cargo', 'build', '--release', '--locked'], 1800),
        ('deny', ['cargo', 'deny', 'check'], 600),
    ]:
        row = {'gate': name, 'command': command, 'started': time.time(), 'status': 'running', 'log': str(OUT / (name + '.log'))}
        manifest['commands'].append(row)
        save()
        with open(row['log'], 'wb') as output:
            active = subprocess.Popen(command, cwd=ROOT, env=ENV, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = active.wait(timeout=deadline)
            except subprocess.TimeoutExpired:
                os.killpg(active.pid, signal.SIGKILL)
                active.wait(timeout=10)
                code = 124
            active = None
        row.update(exit_code=code, status='passed' if code == 0 else 'failed', finished=time.time())
        save()
    manifest['after'] = snapshot()
    manifest['compiled_input_scope_unchanged'] = manifest['before'] == manifest['after']
    binary = Path(ENV['CARGO_TARGET_DIR']) / 'release/dracon-sync'
    if binary.exists():
        manifest['binary_path'] = str(binary)
        manifest['binary_sha256'] = hashlib.sha256(binary.read_bytes()).hexdigest()
    manifest['status'] = 'passed' if manifest['compiled_input_scope_unchanged'] and all(row['exit_code'] == 0 for row in manifest['commands']) else 'failed'
except BaseException as error:
    if manifest['status'] != 'cancelled':
        manifest['status'] = 'failed'
    manifest['error'] = str(error)
finally:
    manifest['finished'] = time.time()
    save()
raise SystemExit(0 if manifest['status'] == 'passed' else 1)
