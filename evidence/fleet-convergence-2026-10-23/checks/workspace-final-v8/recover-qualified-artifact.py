"""Recover lost temporary executable; never replace qualification with metadata."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

OUT = Path(__file__).resolve().parent
source = json.loads((OUT / 'source.json').read_text())
qualified = json.loads((OUT / 'receipt.json').read_text())
assert qualified['status'] == 'passed' and qualified['compiled_input_scope_unchanged']
root = Path(source['frozen_root'])
for name, expected in qualified['after'].items():
    path = root / name
    actual = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    assert actual == expected, 'captured source changed; cannot reuse original qualification'
target = Path(source['target_directory'])
assert target.parent.parent == Path('/tmp') and target.parent.name.startswith('fleet-worktree-qualified-')
assert not target.is_symlink()
target.mkdir(parents=True, exist_ok=True, mode=0o700)
env = {**os.environ, 'CARGO_BUILD_JOBS': '2', 'RUST_TEST_THREADS': '1', 'CARGO_TARGET_DIR': str(target)}
record = {'started': time.time(), 'reason': 'original temporary target disappeared',
          'expected_qualified_sha256': qualified['binary_sha256'], 'status': 'running'}
try:
    with (OUT / 'artifact-recovery-build.log').open('wb') as log:
        child = subprocess.Popen(['cargo', 'build', '--release', '--locked'], cwd=root, env=env,
                                 stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=1800)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=10)
            raise
    assert code == 0, 'recovery build failed'
    binary = target / 'release/dracon-sync'
    actual = hashlib.sha256(binary.read_bytes()).hexdigest()
    record['rebuilt_sha256'] = actual
    assert actual == qualified['binary_sha256'], 'different artifact requires fresh qualification, not reused pass'
    durable = Path.home() / '.local/state/dracon/qualified-artifacts' / actual / 'dracon-sync'
    durable.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if durable.exists():
        assert hashlib.sha256(durable.read_bytes()).hexdigest() == actual
    else:
        shutil.copy2(binary, durable)
        durable.chmod(0o755)
    assert hashlib.sha256(durable.read_bytes()).hexdigest() == actual
    record.update(status='passed', durable_binary=str(durable))
except BaseException as error:
    record.update(status='failed', error=str(error))
    raise
finally:
    record['finished'] = time.time()
    (OUT / 'artifact-recovery.json').write_text(json.dumps(record, indent=2) + '\n')
