import datetime, hashlib, json, os, shutil, signal, subprocess, tempfile, time
from pathlib import Path

qualification_root = Path(__file__).resolve().parent
qualified = json.loads((qualification_root/'receipt.json').read_text())
assert qualified['status']=='passed' and qualified['compiled_input_scope_unchanged']
assert all(row['exit_code']==0 for row in qualified['commands'])
source = json.loads((qualification_root/'source.json').read_text())
state = Path(source['state_root'])
assert state.parent==Path('/tmp') and state.name.startswith('fleet-workspace-frozen-')
candidate = state/'qualified-bin/dracon-sync'
target = Path.home() / '.local/bin/dracon-sync'
expected = qualified['binary_sha256']
old_expected = '8be3718f27c1bc6edee5bb38a27d3911fdb1154715848e148dc13009cb5891cb'
executed = json.loads((qualification_root/'candidate-execution-receipt.json').read_text())
assert executed['ok'] and executed['binary_sha256']==expected
assert executed['concurrent_publication_observed'] and executed['a_still_blocked_while_b_published']
assert executed['orphan_pids_after_shutdown']==[] and executed['shutdown']=='joined after SIGTERM'
receipt = qualification_root/'deployment.json'
def digest(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def pid():
    return int(subprocess.check_output(['systemctl','show','dracon-sync-operator.service','-p','MainPID','--value'], text=True, timeout=10).strip())
def replace(source):
    fd, name = tempfile.mkstemp(prefix='.dracon-sync-qualified-', dir=target.parent)
    try:
        with os.fdopen(fd, 'wb') as out, open(source, 'rb') as inp:
            shutil.copyfileobj(inp, out); out.flush(); os.fsync(out.fileno())
        os.chmod(name, 0o755)
        os.replace(name, target)
    finally:
        if os.path.exists(name): os.unlink(name)
assert digest(candidate) == expected
old_pid = pid()
assert old_pid > 0 and digest(f'/proc/{old_pid}/exe') == old_expected
assert digest(target) == old_expected and target.is_file() and not target.is_symlink()
fd = os.pidfd_open(old_pid)
assert pid() == old_pid and digest(f'/proc/{old_pid}/exe') == old_expected
backup = Path.home()/'.local/state/dracon/deployments'/datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ-final-v6-convergence')
backup.mkdir(parents=True, exist_ok=False)
shutil.copy2(target, backup/'dracon-sync')
assert digest(backup/'dracon-sync') == old_expected
started = time.time()
try:
    replace(candidate)
    assert digest(target) == expected
    signal.pidfd_send_signal(fd, signal.SIGTERM)
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        new_pid = pid()
        if new_pid > 0 and new_pid != old_pid:
            try:
                if digest(f'/proc/{new_pid}/exe') == expected:
                    result = {'started_unix':started, 'verified_unix':time.time(), 'old_pid':old_pid, 'new_pid':new_pid, 'old_loaded_sha256':old_expected, 'loaded_sha256':expected, 'installed_sha256':digest(target), 'backup':str(backup), 'method':'maintenance wrapper; atomic binary replacement; pidfd SIGTERM; Restart=always', 'service_stop_used':False, 'manual_repository_pushes':False}
                    receipt.write_text(json.dumps(result, indent=2)+'\n')
                    print(json.dumps(result, indent=2)); break
            except FileNotFoundError: pass
        time.sleep(0.25)
    else: raise RuntimeError('qualified executable did not become the loaded service within 90 seconds')
except BaseException:
    replace(backup/'dracon-sync')
    current = pid()
    if current > 0:
        try:
            if digest(f'/proc/{current}/exe') == expected:
                rollback_fd = os.pidfd_open(current)
                try:
                    if pid()==current and digest(f'/proc/{current}/exe')==expected:
                        signal.pidfd_send_signal(rollback_fd, signal.SIGTERM)
                finally:
                    os.close(rollback_fd)
        except ProcessLookupError: pass
    raise
finally:
    os.close(fd)
