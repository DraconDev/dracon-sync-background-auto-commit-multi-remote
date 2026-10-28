#!/usr/bin/env python3
"""Linux/x86_64 disposable actual-helper qualification. No production mutations.
Stops the owned helper at real renameat2 entry/exit using ptrace, never patches
registers or source. Baseline success is required before adversarial scheduling.
A preservation failure is reported as failure, not accepted as an expected pass.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import sys
import tempfile
import time
import traceback

LIBC = ctypes.CDLL(None, use_errno=True)
LIBC.ptrace.restype = ctypes.c_long
TRACEME, GETREGS, SYSCALL, SETOPTIONS = 0, 12, 24, 0x4200
TRACESYSGOOD, EXITKILL = 1, 0x100000
RENAMEAT2 = 316
INHERITED_FENCE_ENV = 'DRACON_CANONICAL_WRITER_FD_V1'

class Registers(ctypes.Structure):
    _fields_ = [(name, ctypes.c_ulonglong) for name in (
        'r15 r14 r13 r12 rbp rbx r11 r10 r9 r8 rax rcx rdx rsi rdi '
        'orig_rax rip cs eflags rsp ss fs_base gs_base ds es fs gs').split()]

def ptrace(op, pid, address=0, data=0):
    ctypes.set_errno(0)
    result = LIBC.ptrace(op, pid, ctypes.c_void_p(address), data)
    if result == -1 and ctypes.get_errno():
        raise OSError(ctypes.get_errno(), 'owned-child ptrace')
    return result

def environment():
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
    env.update(GIT_CONFIG_GLOBAL='/dev/null', GIT_CONFIG_NOSYSTEM='1', TOKIO_WORKER_THREADS='2')
    return env

def git(repo, *args):
    out = subprocess.run(['git', '--no-optional-locks', '-C', str(repo), *args],
                         env=environment(), capture_output=True, timeout=10, check=True)
    return out.stdout

def fingerprint(path):
    st = path.stat()
    return {'identity': [st.st_dev, st.st_ino, st.st_size, st.st_mode,
                         st.st_uid, st.st_gid, st.st_mtime_ns // 10**9, st.st_mtime_ns % 10**9],
            'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}

def fixture(parent, helper):
    repo = parent / 'repo'
    repo.mkdir()
    git(repo, 'init', '--template=', '-q', '-b', 'main')
    git(repo, 'config', 'user.name', 'test')
    git(repo, 'config', 'user.email', 'test@test')
    git(repo, 'config', 'commit.gpgsign', 'false')
    hooks = repo / '.git' / 'fixture-hooks'
    hooks.mkdir()
    git(repo, 'config', 'core.hooksPath', str(hooks))
    source = repo / 'sample.txt'
    source.write_bytes(b'HEAD payload retained\n')
    source.chmod(0o600)
    git(repo, 'add', '--', 'sample.txt')
    git(repo, 'commit', '-q', '-m', 'disposable fixture')
    head = git(repo, 'rev-parse', 'HEAD').decode().strip()
    tree = git(repo, 'rev-parse', 'HEAD^{tree}').decode().strip()
    verify = subprocess.run([str(helper), 'storage', 'verify-configured-tree', '--repo', str(repo),
                             '--tree', tree, '--index', str(repo / '.git' / 'index')],
                            env=environment(), capture_output=True, timeout=15, check=True)
    tokens = verify.stdout.decode().split()
    assert len(tokens) == 3 and tokens[1:] == [tree, 'ordinary'], verify.stdout
    recovery = Path(tempfile.mkdtemp(prefix='dracon-excluded-recovery-', dir=repo / '.git'))
    recovery.chmod(0o700)
    files = recovery / 'files'
    files.mkdir(mode=0o700)
    desired = files / 'sample.txt'
    desired.write_bytes(source.read_bytes())
    desired.chmod(0o600)
    source.write_bytes(b'operator ORIGINAL payload\n')
    request = {'source_id': tokens[0], 'repo': list(os.fsencode(repo)), 'head': head,
               'targets': [{'path': list(b'sample.txt'), 'before': fingerprint(source),
                            'desired': fingerprint(desired)}]}
    request_path = recovery / 'recovery.json'
    request_path.write_text(json.dumps(request))
    request_path.chmod(0o600)
    return repo, source, desired, request_path, request

def baseline(helper, parent):
    repo, source, desired, request_path, request = fixture(parent, helper)
    result = subprocess.run([str(helper), '--dracon-excluded-restore-helper', str(request_path), str(os.getpid())],
                            env=environment(), capture_output=True, timeout=15, check=True)
    assert json.loads(result.stdout) == [request['source_id'], True], result.stdout
    assert source.read_bytes() == b'HEAD payload retained\n'
    assert desired.read_bytes() == b'operator ORIGINAL payload\n'
    return {'response': json.loads(result.stdout), 'canonicalSha256': fingerprint(source)['sha256'],
            'displacedSha256': fingerprint(desired)['sha256'], 'pass': True}

def adversarial(helper, parent, expect_final=True, warden=None):
    repo, source, desired, request_path, request = fixture(parent, helper)
    before_index = (repo / '.git' / 'index').read_bytes()
    stdout_path, stderr_path = parent / 'helper.stdout', parent / 'helper.stderr'
    output = open(stdout_path, 'wb')
    error = open(stderr_path, 'wb')
    alive, queued, newer_fd = True, None, None
    pid = os.fork()
    if pid == 0:
        try:
            os.setsid()
            os.dup2(output.fileno(), 1)
            os.dup2(error.fileno(), 2)
            ptrace(TRACEME, 0)
            os.execve(str(helper), [str(helper), '--dracon-excluded-restore-helper', str(request_path), str(os.getppid())], environment())
        except BaseException as exc:
            os.write(2, repr(exc).encode())
            os._exit(111)
    output.close()
    error.close()
    observations = {'expectFinalResponse': expect_final,
                    'writerMode': 'uncooperative-raw-open' if warden is None else 'independent-warden-fenced-writer',
                    'helperPid': pid, 'requestOriginalInode': request['targets'][0]['before']['identity'][1],
                    'requestDesiredInode': request['targets'][0]['desired']['identity'][1], 'exchangeStops': []}
    try:
        _, status = os.waitpid(pid, 0)
        assert os.WIFSTOPPED(status), ('helper failed before tracing', status, stderr_path.read_text())
        ptrace(SETOPTIONS, pid, 0, TRACESYSGOOD | EXITKILL)
        ptrace(SYSCALL, pid)
        exchanged = 0
        while True:
            _, status = os.waitpid(pid, 0)
            if os.WIFEXITED(status) or os.WIFSIGNALED(status):
                alive = False
                observations['helperWaitStatus'] = status
                break
            assert os.WIFSTOPPED(status), status
            stopped = os.WSTOPSIG(status)
            if stopped == signal.SIGTRAP | 0x80:
                registers = Registers()
                ptrace(GETREGS, pid, 0, ctypes.byref(registers))
                entry = ctypes.c_longlong(registers.rax).value == -38
                if registers.orig_rax == RENAMEAT2 and registers.r8 == 2:
                    observations['exchangeStops'].append({'entry': entry, 'return': ctypes.c_longlong(registers.rax).value})
                    if entry and exchanged == 0 and queued is None:
                        marker, go = parent / 'old-open.json', parent / 'old-go'
                        writer = (
                            'import os,sys,json,time;from pathlib import Path;'
                            'f=os.open(sys.argv[1],os.O_WRONLY);s=os.fstat(f);'
                            'Path(sys.argv[2]).write_text(json.dumps({"pid":os.getpid(),"inode":s.st_ino}));'
                            '\nwhile not Path(sys.argv[3]).exists():time.sleep(.002)\n'
                            'os.pwrite(f,b"OLD-WRITER",0);os.fsync(f);os.close(f)')
                        writer_command = [sys.executable, '-c', writer, str(source), str(marker), str(go)]
                        writer_env = environment()
                        if warden is None:
                            queued = subprocess.Popen(writer_command, env=writer_env, start_new_session=True)
                            deadline = time.monotonic() + 3
                            while True:
                                locks = Path('/proc/locks').read_text().splitlines()
                                rows = [line for line in locks if str(pid) in line.split()
                                        and any(part.endswith(':' + str(source.stat().st_ino)) for part in line.split())]
                                if any('BREAKING' in row for row in rows):
                                    observations['actualBreakingLeaseRows'] = rows
                                    break
                                if time.monotonic() >= deadline:
                                    raise AssertionError(('actual old-inode open did not request lease break', rows))
                                time.sleep(.002)
                        else:
                            writer_env.pop(INHERITED_FENCE_ENV, None)
                            queued = subprocess.Popen([
                                str(warden), 'write-fence', '--repo', str(repo),
                                '--wait-secs', '30', '--run-secs', '20', '--', *writer_command,
                            ], env=writer_env, start_new_session=True)
                            observations['writerFencePid'] = queued.pid
                            deadline = time.monotonic() + 0.2
                            while time.monotonic() < deadline:
                                assert queued.poll() is None, 'fenced writer exited before acquiring the helper-held worktree fence'
                                assert not marker.exists(), 'fenced writer opened before acquiring the worktree fence'
                                time.sleep(.005)
                            observations['writerWaitedAtFence'] = True
                        assert not marker.exists(), 'old writer unexpectedly opened before helper publication settled'
                    if not entry:
                        assert registers.rax == 0, ('actual renameat2 failed', registers.rax)
                        exchanged += 1
                        if exchanged == 1:
                            # New canonical inode is exposed but helper has not
                            # executed ONE userspace instruction after exchange.
                            newer_fd = os.open(source, os.O_RDWR | os.O_NONBLOCK)
                            observations['newerOpenedInode'] = os.fstat(newer_fd).st_ino
                            observations['canonicalAtNewOpen'] = source.stat().st_ino
                ptrace(SYSCALL, pid)
            else:
                # Forward real signals; SIGIO is ignored by the actual helper.
                ptrace(SYSCALL, pid, 0, 0 if stopped == signal.SIGTRAP else stopped)
        observations['helperStdout'] = stdout_path.read_text()
        observations['helperStderr'] = stderr_path.read_text()
        assert exchanged == 1 and newer_fd is not None, ('schedule did not reach the single exchange boundary', observations)
        assert json.loads(observations['helperStdout']) == [request['source_id'], expect_final], observations['helperStdout']
        deadline = time.monotonic() + 3
        while not marker.exists():
            assert time.monotonic() < deadline, 'old queued open did not finish after lease release'
            time.sleep(.002)
        observations['oldWriter'] = json.loads(marker.read_text())
        # Observe both writers independently. Retaining bytes privately does
        # not prove the promised canonical writer continuity; do not invert
        # this requirement just because the candidate displaces the old writer.
        observations['oldWriterLandedOnCanonical'] = source.stat().st_ino == observations['oldWriter']['inode']
        observations['oldWriterLandedDisplaced'] = desired.stat().st_ino == observations['oldWriter']['inode']
        go.touch()
        assert queued.wait(timeout=5) == 0
        os.pwrite(newer_fd, b'NEW-WRITER', 0)
        os.fsync(newer_fd)
        observations['canonicalAfterBothWrites'] = source.read_bytes().hex()
        observations['privateAfterBothWrites'] = desired.read_bytes().hex()
        observations['indexPreserved'] = (repo / '.git' / 'index').read_bytes() == before_index
        observations['newerWriterStillCanonical'] = source.stat().st_ino == os.fstat(newer_fd).st_ino
        observations['canonicalContainsNewerWrite'] = source.read_bytes().startswith(b'NEW-WRITER')
        return observations
    finally:
        if newer_fd is not None:
            os.close(newer_fd)
        if alive:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        if queued is not None and queued.poll() is None:
            if warden is None:
                queued.kill()
            else:
                try: os.killpg(queued.pid, signal.SIGKILL)
                except ProcessLookupError: pass
            queued.wait(timeout=5)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--helper', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--expect-final', choices=['true', 'false'], default='true',
                        help='expected helper finalization response')
    parser.add_argument('--warden', type=Path,
                        help='optional Warden CLI for the independent cooperative writer; default keeps the raw negative control')
    args = parser.parse_args()
    helper = args.helper.resolve()
    warden = args.warden.resolve() if args.warden else None
    if warden is not None:
        assert warden.is_file() and os.access(warden, os.X_OK), f'Warden CLI is not executable: {warden}'
        assert INHERITED_FENCE_ENV not in os.environ, 'run the qualification driver outside any outer writer fence'
    record = {'scope': 'actual isolated helper / owned disposable processes only; no deployment/fleet claim',
              'helper': str(helper), 'helperSha256': hashlib.sha256(helper.read_bytes()).hexdigest(),
              'writerMode': 'uncooperative-raw-open' if warden is None else 'independent-warden-fenced-writer',
              'warden': str(warden) if warden else None,
              'wardenSha256': hashlib.sha256(warden.read_bytes()).hexdigest() if warden else None,
              'contract': 'Both accepted writer FDs retain canonical identity; private retention alone is not a pass.'}
    def timeout(_signum, _frame):
        raise TimeoutError('finite owned-helper qualification deadline expired')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(60)
    try:
        assert platform.system() == 'Linux' and platform.machine() == 'x86_64', 'unsupported qualification architecture'
        with tempfile.TemporaryDirectory(prefix='dracon-exchange-syscall-') as root:
            root = Path(root)
            for name in ['baseline', 'adversarial']:
                (root / name).mkdir()
            record['baseline'] = baseline(helper, root / 'baseline')
            record['adversarial'] = adversarial(helper, root / 'adversarial', args.expect_final == 'true', warden)
            a = record['adversarial']
            record['pass'] = (a['newerWriterStillCanonical']
                              and a['canonicalContainsNewerWrite']
                              and a['indexPreserved']
                              and a['oldWriterLandedOnCanonical'])
    except BaseException as exc:
        record['error'] = repr(exc)
        record['traceback'] = traceback.format_exc()
        record['pass'] = False
    finally:
        signal.alarm(0)
        args.output.write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record, indent=2))
    return 0 if record['pass'] else 1

if __name__ == '__main__':
    sys.exit(main())
