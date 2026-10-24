#!/usr/bin/env python3
"""One-episode recovery: never a general process-name reaper or push tool."""
import json, os, signal, subprocess, sys, time
from pathlib import Path

OUT = Path(__file__).parent / 'stale-operation-cancellation-d3CKuT.json'
BASE = Path('/home/dracon/.local/state/dracon/sync/gates/browser-extensions-shared')
ACTIVE = BASE / 'dracon-pre-push.d3CKuT'
FAILED = BASE / 'dracon-pre-push.Dy32YA'
REPO = '/home/dracon/Dev/browser-extensions-shared'
UNIT = 'dracon-sync-operator.service'
report = {'observed_at': time.time(), 'active_manifest': str(ACTIVE / 'manifest.json'), 'signals': []}


def save():
    OUT.write_text(json.dumps(report, indent=2) + '\n')


def info(pid):
    proc = Path('/proc') / str(pid)
    fields = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
    return {'pid': pid, 'state': fields[0], 'ppid': int(fields[1]), 'group': int(fields[2]),
            'starttime': fields[19], 'uid': proc.stat().st_uid,
            'cwd': str((proc / 'cwd').resolve()), 'unit': UNIT in (proc / 'cgroup').read_text(),
            'name': (proc / 'comm').read_text().strip()}


def members(group):
    result = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdecimal():
            continue
        try:
            item = info(int(proc.name))
            if item['group'] == group and item['state'] not in ('Z', 'X'):
                result.append(item)
        except (OSError, ProcessLookupError):
            pass
    return result


def scoped(item):
    return item['uid'] == os.geteuid() and item['unit'] and (
        item['cwd'] == REPO or item['cwd'].startswith(str(ACTIVE) + '/'))


try:
    active = json.loads((ACTIVE / 'manifest.json').read_text())
    failed = json.loads((FAILED / 'manifest.json').read_text())
    assert active['repo'] == failed['repo'] == REPO
    assert failed['status'] in ('running', 'failed', 'cancelled')
    target = {r['sha'] for r in active['revisions']}
    assert target == {r['sha'] for r in failed['revisions']} and len(target) == 1
    assert any(g['status'] == 'failed' for r in failed['revisions'] for g in r.get('gates', []))
    sha = next(iter(target)); report['failed_revision'] = sha
    if active['status'] != 'running':
        report['result'] = 'already terminal; no signals'; save(); sys.exit(0)
    owner = info(active['ownerPid']); group = owner['group']
    assert scoped(owner) and owner['cwd'] == REPO
    owner_args = (Path('/proc') / str(owner['pid']) / 'cmdline').read_bytes().split(b'\0')
    assert (REPO + '/scripts/git-hooks/run-push-gates.mjs').encode() in owner_args
    main = int(subprocess.check_output(['systemctl', 'show', UNIT, '--property=MainPID', '--value']))
    assert main > 1 and group != os.getpgid(main) and group != os.getpgrp()
    leader = info(group); assert scoped(leader) and leader['name'] == 'git'
    args = (Path('/proc') / str(group) / 'cmdline').read_bytes().split(b'\0')
    assert b'push' in args and any(a.startswith((sha + ':').encode()) for a in args)
    assert subprocess.run(['git', '-C', REPO, 'merge-base', '--is-ancestor',
                           '5d8e0ac02d374edeb17aa60dffb63f7c754dba05', 'HEAD']).returncode == 0
    initial = members(group); assert initial and all(scoped(item) for item in initial)
    report.update(group=group, daemon_pid=main, initial_members=initial,
                  applied='--apply' in sys.argv, corrected_test_anchor='5d8e0ac02d374edeb17aa60dffb63f7c754dba05')
    save()
    if '--apply' not in sys.argv:
        print('Validated isolated superseded operation; no signals in inspection mode.')
        sys.exit(0)
    deadline = time.monotonic() + 10
    while True:
        current = members(group)
        if not current:
            report['result'] = 'no live members remain'; break
        assert all(scoped(item) for item in current), 'scope changed; no further signals allowed'
        sig = signal.SIGTERM if time.monotonic() < deadline - 5 else signal.SIGKILL
        for item in current:
            try:
                fd = os.pidfd_open(item['pid'])
                try:
                    now = info(item['pid'])
                    assert now['starttime'] == item['starttime'] and now['group'] == group and scoped(now)
                    signal.pidfd_send_signal(fd, sig)
                    report['signals'].append({'pid': item['pid'], 'starttime': item['starttime'], 'signal': sig.name})
                finally:
                    os.close(fd)
            except ProcessLookupError:
                pass
        save()
        if time.monotonic() >= deadline:
            report['result'] = 'cleanup deadline reached; live members require assessment'; break
        time.sleep(0.1)
    report['remaining'] = members(group)
    report['manifest_after'] = json.loads((ACTIVE / 'manifest.json').read_text())['status']
    report['finished_at'] = time.time(); save()
    assert not report['remaining'], 'not claiming cleanup'
    print('Only the verified superseded publication family was cancelled; no live members remain.')
except BaseException as error:
    if not isinstance(error, SystemExit):
        report['error'] = repr(error); save()
    raise
