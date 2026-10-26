#!/usr/bin/env python3
"""Deploy the QUALIFIED no-rollback dracon-sync binary.

Atomic replacement + pidfd SIGTERM + Restart=always. Never `systemctl stop`.
Refuses unless the candidate is byte-identical to the qualified release build,
and rolls back to the saved previous binary if the new executable does not
become the loaded process within the deadline.
"""
import datetime, hashlib, json, os, shutil, signal, subprocess, tempfile, time
from pathlib import Path

root = Path(__file__).resolve().parent
stat = json.loads((root.parent / 'status-inspection-root-cause' / 'statfix-receipt.json').read_text())
norollback = json.loads((root / 'norollback-qualified-v3-receipt.json').read_text())
expected = stat['release_binary_sha256']
assert stat['gates']['workspace_tests'].endswith('exit=0'), 'workspace gate not green'
assert stat['gates']['release_build'].endswith('exit=0'), 'release build not green'
assert stat['gates']['clippy'].endswith('exit=0'), 'clippy not green'
assert stat['gates']['deny'].endswith('exit=0'), 'deny not green'
assert 'pass=true' in norollback['regressions']['actual_syscall_release_binary'], 'syscall qualification not green'

candidate = Path('/home/dracon/Dev/dracon-utilities/target/release/dracon-sync')
target = Path.home() / '.local/bin/dracon-sync'

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def pid():
    out = subprocess.check_output(
        ['systemctl', 'show', 'dracon-sync-operator.service', '-p', 'MainPID', '--value'],
        text=True, timeout=10).strip()
    return int(out or 0)

assert digest(candidate) == expected, f'candidate {digest(candidate)} is not the qualified artifact {expected}'
old_pid = pid()
assert old_pid > 0, 'daemon is not running'
old_sha = digest(f'/proc/{old_pid}/exe')
assert old_sha != expected, 'already running the qualified build'
assert digest(target) == old_sha, 'installed binary differs from the running one; refusing to guess'
assert target.is_file() and not target.is_symlink()

backup = Path.home() / '.local/state/dracon/deployments' / (
    datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-status-inspect-fix')
backup.mkdir(parents=True, exist_ok=False)
shutil.copy2(target, backup / 'dracon-sync')
assert digest(backup / 'dracon-sync') == old_sha

def replace(source):
    fd, name = tempfile.mkstemp(prefix='.dracon-sync-qualified-', dir=target.parent)
    try:
        with os.fdopen(fd, 'wb') as out, open(source, 'rb') as inp:
            shutil.copyfileobj(inp, out)
            out.flush()
            os.fsync(out.fileno())
        os.chmod(name, 0o755)
        os.replace(name, target)
    finally:
        if os.path.exists(name):
            os.unlink(name)

fd = os.pidfd_open(old_pid)
result = {'started_unix': time.time(), 'old_pid': old_pid, 'old_loaded_sha256': old_sha,
          'expected_sha256': expected, 'backup': str(backup), 'service_stop_used': False,
          'method': 'atomic binary replacement; pidfd SIGTERM; Restart=always'}
try:
    replace(candidate)
    assert digest(target) == expected
    signal.pidfd_send_signal(fd, signal.SIGTERM)
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        new_pid = pid()
        if new_pid > 0 and new_pid != old_pid:
            try:
                if digest(f'/proc/{new_pid}/exe') == expected:
                    result.update({'new_pid': new_pid, 'loaded_sha256': expected,
                                   'installed_sha256': digest(target),
                                   'verified_unix': time.time(), 'ok': True})
                    break
            except FileNotFoundError:
                pass
        time.sleep(0.25)
    else:
        raise RuntimeError('qualified executable did not become the loaded service within 120s')
except BaseException:
    replace(backup / 'dracon-sync')
    current = pid()
    if current > 0:
        try:
            if digest(f'/proc/{current}/exe') == expected:
                rfd = os.pidfd_open(current)
                try:
                    if pid() == current and digest(f'/proc/{current}/exe') == expected:
                        signal.pidfd_send_signal(rfd, signal.SIGTERM)
                finally:
                    os.close(rfd)
        except ProcessLookupError:
            pass
    raise
finally:
    os.close(fd)

(root.parent / 'status-inspection-root-cause' / 'deployment.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
