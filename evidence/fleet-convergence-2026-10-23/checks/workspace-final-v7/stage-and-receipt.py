"""Stage only a fully qualified capture; execute private transports only."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess

OUT = Path(__file__).resolve().parent
ROOT = Path('/home/dracon/Dev/dracon-utilities')
q = json.loads((OUT / 'receipt.json').read_text())
source = json.loads((OUT / 'source.json').read_text())
assert q['status'] == 'passed' and q['compiled_input_scope_unchanged']
assert all(row['exit_code'] == 0 for row in q['commands'])
state = Path(source['state_root'])
assert state.parent == Path('/tmp') and state.name.startswith('fleet-worktree-qualified-')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def identity():
    pid = int(subprocess.check_output(['systemctl', 'show', 'dracon-sync-operator.service', '-p', 'MainPID', '--value'], text=True, timeout=10))
    return {'pid': pid, 'sha256': digest(f'/proc/{pid}/exe')}


for name, expected in source['overlay'].items():
    path = Path(source['frozen_root']) / name
    assert (digest(path) if path.is_file() else None) == expected
filtered = []
for name, metadata in source['repositories'].items():
    checkout = metadata['private_checkout']
    assert subprocess.check_output(['git', '-C', checkout, 'rev-parse', 'HEAD'], text=True, timeout=30).strip() == metadata['ref']
    differences = subprocess.run(['git', '-C', checkout, 'diff', '--name-only', '-z', 'HEAD', '--'], capture_output=True, timeout=180)
    assert differences.returncode == 0
    for encoded in differences.stdout.split(b'\0'):
        if not encoded:
            continue
        relative = os.fsdecode(encoded)
        path = Path(checkout) / relative
        overlay_key = str(path.relative_to(Path(source['frozen_root'])))
        if overlay_key in source['overlay']:
            continue  # Exact captured bytes checked above; not falsely Git-ref equality.
        assert path.is_file() and not path.is_symlink()
        ordinary = subprocess.run(['git', '-C', checkout, 'cat-file', '--filters', '--path=' + relative, 'HEAD:' + relative], capture_output=True, timeout=180)
        assert ordinary.returncode == 0 and path.read_bytes() == ordinary.stdout
        filtered.append({'repo': name, 'path': relative, 'required_smudge_byte_equivalence': True})
(OUT / 'frozen-input-integrity.json').write_text(json.dumps({'refs_unchanged': True, 'captured_overlay_unchanged': True, 'other_tracked_inputs_match_required_smudge': True, 'filtered_equivalents': filtered}, indent=2) + '\n')
before = identity()
assert before['sha256'] == '59ef460ef335d51aafd09f01d7537589a70712254feb2557751c440f62146cfc'
original = Path(q['binary_path'])
assert digest(original) == q['binary_sha256']
candidate = state / 'qualified-bin/dracon-sync'
candidate.parent.mkdir(mode=0o700)
assert not candidate.exists()
shutil.copy2(original, candidate)
candidate.chmod(0o755)
assert digest(candidate) == q['binary_sha256']
dependencies = subprocess.run(['ldd', str(candidate)], capture_output=True, text=True, timeout=20)
(OUT / 'runtime-dependencies.log').write_text(dependencies.stdout + dependencies.stderr)
assert str(state) not in dependencies.stdout, 'disposable runtime library dependency'
script = OUT / 'isolated-publication-receipt.py'
assert not script.exists()
shutil.copyfile(ROOT / 'dracon-sync/scripts/isolated-publication-receipt.py', script)
script_hash = digest(script)
execution = state / 'candidate-execution'
assert not execution.exists()
owner = subprocess.Popen(['python3', str(script), str(candidate), str(execution)], start_new_session=True)
try:
    code = owner.wait(timeout=2400)
except subprocess.TimeoutExpired:
    # The receipt's SIGTERM handler enters its bounded finally and joins its
    # separate private daemon group. Do not SIGKILL that cleanup owner.
    owner.send_signal(signal.SIGTERM)
    owner.wait(timeout=210)
    raise RuntimeError('candidate receipt exceeded bounded execution envelope')
assert code == 0
receipt = json.loads((execution / 'receipt.json').read_text())
assert receipt['ok'] and receipt['binary_sha256'] == q['binary_sha256']
assert receipt['a_completed_worker_retired'] and receipt['a_post_round_work_published']
assert receipt['orphan_pids_after_shutdown'] == [] and receipt['shutdown'] == 'joined after SIGTERM'
after = identity()
assert before == after, 'candidate fixtures changed the real loaded daemon'
assert digest(script) == script_hash
shutil.copyfile(execution / 'receipt.json', OUT / 'candidate-execution-receipt.json')
(OUT / 'candidate-stage.json').write_text(json.dumps({'candidate': str(candidate), 'binary_sha256': digest(candidate), 'receipt_script_sha256': script_hash, 'real_service_before': before, 'real_service_after': after, 'fixture_root_outside_watch_roots': str(execution)}, indent=2) + '\n')
