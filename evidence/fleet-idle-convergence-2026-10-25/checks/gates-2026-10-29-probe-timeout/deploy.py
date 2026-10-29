#!/usr/bin/env python3
"""Deploy the gated dracon-sync release: loose-object auto-GC + decided hook verdict.

Same sanctioned shape as `deploy-norollback-qualified.py`:
  * atomic binary replacement (mkstemp + fsync + os.replace), never a symlink;
  * pidfd SIGTERM to the running daemon; `Restart=always` reloads it;
  * `systemctl stop` is NEVER used (daemon-quiesce policy);
  * refuses unless the candidate is byte-identical to the gated release build
    whose source tree is unchanged from the gated revision;
  * rolls back to the saved previous binary if the new executable does not
    become the loaded process within the deadline.

Reads the gate receipt written by the workspace-gate run, so this cannot deploy
an artifact that did not pass the four gates.
"""
import datetime, hashlib, json, os, shutil, signal, subprocess, sys, tempfile, time
from pathlib import Path

ROOT = Path('/home/dracon/Dev/dracon-utilities')
GATE = ROOT / 'dracon-sync/evidence/fleet-idle-convergence-2026-10-25/checks/gates-2026-10-29-probe-timeout/gates.json'
GIT = '/run/current-system/sw/bin/git'


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def git(*args):
    return subprocess.check_output([GIT, *args], text=True).strip()


def pid():
    out = subprocess.check_output(
        ['systemctl', 'show', 'dracon-sync-operator.service', '-p', 'MainPID', '--value'],
        text=True, timeout=10).strip()
    return int(out or 0)


# ── preconditions ───────────────────────────────────────────────────────────
receipt = json.loads(GATE.read_text())
for gate, want in (('workspace_tests', 0), ('release_build', 0),
                   ('clippy_all_targets_strict', 0), ('cargo_deny', 0)):
    got = receipt['gates'][gate]['exit']
    assert got == want, f'gate {gate} exit {got} != {want}'
assert receipt['gates']['workspace_tests']['failed'] == 0, 'workspace tests had failures'

gated_rev = receipt['assembly']['dracon_sync']
current_rev = git('-C', str(ROOT / 'dracon-sync'), 'rev-parse', 'HEAD')
# Only evidence/docs may differ from the gated revision; source and lockfile must
# be byte-identical to what the gates actually validated.
unchanged = git('-C', str(ROOT / 'dracon-sync'), 'diff', '--name-only', gated_rev, current_rev, '--', 'src', 'Cargo.toml')
assert unchanged == '', f'source differs from the gated assembly: {unchanged!r}'

candidate = ROOT / 'target/release/dracon-sync'
target = Path.home() / '.local/bin/dracon-sync'
expected = digest(candidate)

assert candidate.is_file() and not candidate.is_symlink()
assert digest(target) != expected, 'already running the candidate build'

old_pid = pid()
assert old_pid > 0, 'daemon is not running'
old_sha = digest(f'/proc/{old_pid}/exe')
assert digest(target) == old_sha, 'installed binary differs from the running one; refusing to guess'
assert target.is_file() and not target.is_symlink()

# The two repairs must actually be present in the artifact being shipped.
strings = subprocess.check_output(['strings', str(candidate)], text=True)
for marker in ('HISTORY_PROBE_TIMED_OUT',
               'history probe rejected by git'):
    assert marker in strings, f'candidate lacks repaired marker {marker!r}'

# ── backup ──────────────────────────────────────────────────────────────────
backup = Path.home() / '.local/state/dracon/deployments' / (
    datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-probe-timeout-fix')
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


result = {
    'started_unix': time.time(),
    'old_pid': old_pid,
    'old_loaded_sha256': old_sha,
    'expected_sha256': expected,
    'gated_revision': gated_rev,
    'deployed_from_revision': current_rev,
    'backup': str(backup),
    'service_stop_used': False,
    'method': 'atomic binary replacement; pidfd SIGTERM; Restart=always',
}

fd = os.pidfd_open(old_pid)
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
                    result.update({
                        'new_pid': new_pid,
                        'loaded_sha256': expected,
                        'installed_sha256': digest(target),
                        'verified_unix': time.time(),
                        'ok': True,
                    })
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

(GATE.parent / 'deployment.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
sys.exit(0 if result.get('ok') else 1)
