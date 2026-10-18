#!/usr/bin/env python3
"""Pinned, bounded one-host migration; does not run the daemon as root.
Prepare unprivileged; review frozen files; administrator invokes --apply.
Rollback is automatic on failure. No credentials or repository contents copied.
NixOS note: /etc/systemd/system is an immutable symlink into /nix/store, so
system units install to root-writable runtime /run/systemd/system (ephemeral
across reboot; a declarative NixOS systemd.services module is the persistent
follow-up). No /etc writes are attempted.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

HOME = Path('/home/dracon')
UID, GID = 1000, 100
UNIT = 'dracon-sync-operator.service'
WATCH = 'dracon-sync-operator-watchdog'
CONFIG = HOME / '.config/systemd/user'
# NixOS: /etc/systemd/system -> /etc/static/systemd/system -> /nix/store (read-only).
# Root-writable system-manager runtime directory; ephemeral across reboot.
SYSTEM = Path('/run/systemd/system')
BIN = HOME / '.local/bin/dracon-sync'
EXPECTED_BINARY = '2b74bb7c84cb9572b35b0ee246bdd29a9e9e56d622fe70fbe46779678d4dfdba'
TOOLS = '/run/current-system/sw/bin'
ENV = {**os.environ, 'PATH': TOOLS + ':/run/wrappers/bin', 'HOME': str(HOME)}

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def run(args, check=True, seconds=45, extra_env=None):
    env = {**ENV, **extra_env} if extra_env else ENV
    return subprocess.run([str(a) for a in args], env=env, check=check,
                          text=True, capture_output=True, timeout=seconds)

def user(*args, check=True):
    return run(['runuser', '-u', 'dracon', '--', 'env', 'HOME='+str(HOME),
                'XDG_RUNTIME_DIR=/run/user/1000', 'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus',
                *args], check=check)

def userctl(*args, check=True):
    return user(TOOLS+'/systemctl', '--user', *args, check=check)

def freeze_markers():
    return [HOME/'.dracon/dracon-sync.freeze', HOME/'.dracon/freeze/dracon-sync']

def require_stopped(control, name):
    state = control('show', name, '-p', 'ActiveState', '-p', 'MainPID', '-p', 'ControlPID').stdout
    fields = dict(line.split('=', 1) for line in state.splitlines() if '=' in line)
    assert fields.get('ActiveState') in ('inactive', 'failed'), 'shutdown not confirmed: '+name
    if name.endswith('.service'):
        assert fields.get('MainPID') == '0' and fields.get('ControlPID') == '0', 'process still present: '+name

def systemctl(*args):
    return run([TOOLS+'/systemctl', *args])

def dump_failure(dest, label, error):
    """Persist captured child output for a failed step; rollback still proceeds."""
    body = ('failed step: '+label+'\nreturncode: '+str(getattr(error, 'returncode', '?'))+
            '\n--- stdout ---\n'+(getattr(error, 'stdout', None) or '')+
            '\n--- stderr ---\n'+(getattr(error, 'stderr', None) or '')+'\n')
    try:
        with open(dest/'handover-failure.log', 'a') as handle:
            handle.write(body)
        os.chown(dest/'handover-failure.log', UID, GID)
    except OSError as log_error:
        body += '(log write failed: '+str(log_error)+')\n'
    print(body, file=sys.stderr)

def build_unit(original):
    assert original.count('ExecStart=%h/.local/bin/dracon-sync daemon') == 1
    result = original.replace('%h', str(HOME)).replace('%u', 'dracon')
    result = result.replace('After=default.target', 'After=network-online.target')
    result = result.replace('WantedBy=default.target', 'WantedBy=multi-user.target')
    result += ('\n[Service]\nUser=dracon\nGroup=users\nPrivateUsers=no\n'
               'Environment=HOME=/home/dracon\nEnvironment=XDG_RUNTIME_DIR=/run/user/1000\n'
               'Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus\n'
               'EnvironmentFile=-/run/systemd/system/dracon-sync-operator.environment\n'
               '# Fail before scheduling if root provenance or operator identity is lost.\n'
               'ExecStartPre=/etc/profiles/per-user/dracon/bin/node -e "const s=require(\'node:fs\');if(process.geteuid()!==1000||s.statSync(\'/\').uid!==0||s.statSync(\'/home\').uid!==0)process.exit(78)"\n')
    return result

def prepare(dest):
    assert os.geteuid() == UID
    assert not dest.exists(), 'plan directory must be fresh'
    assert dest.is_absolute() and str(dest).startswith(str(HOME / '.local/state/dracon-sync/service-migration')+'/')
    assert sha(BIN) == EXPECTED_BINARY
    dest.mkdir(parents=True, mode=0o700)
    source = CONFIG / 'dracon-sync.service'
    drops = sorted((CONFIG / 'dracon-sync.service.d').glob('*.conf'))
    assert [p.name for p in drops] == ['10-resource-headroom.conf', '20-bridge-fixture.conf'], 'unexpected drop-in; review before preparing'
    (dest / UNIT).write_text(build_unit(source.read_text()))
    for p in drops:
        shutil.copy2(p, dest / p.name)
    (dest / (WATCH+'.service')).write_text('[Unit]\nDescription=Restart operator-UID sync scheduler if inactive\nConditionPathExists=!/home/dracon/.dracon/dracon-sync.maintenance-hold\n[Service]\nType=oneshot\nExecStart='+TOOLS+'/systemctl start '+UNIT+'\nNoNewPrivileges=true\nProtectSystem=strict\nProtectHome=read-only\n')
    (dest / (WATCH+'.timer')).write_text('[Unit]\nDescription=Operator sync scheduler watchdog (2 minutes)\n[Timer]\nOnBootSec=2min\nOnUnitActiveSec=2min\nAccuracySec=10s\n[Install]\nWantedBy=timers.target\n')
    pid = run([TOOLS+'/systemctl','--user','show','dracon-sync.service','-p','MainPID','--value']).stdout.strip()
    assert int(pid)>0
    env = dict(s.split(b'=',1) for s in Path('/proc/'+pid+'/environ').read_bytes().split(b'\0') if b'=' in s)
    sock = env.get(b'SSH_AUTH_SOCK', b'').decode()
    assert not sock or all(c.isalnum() or c in '/_:.@-' for c in sock), 'unsupported socket path'
    (dest / 'dracon-sync-operator.environment').write_text(('SSH_AUTH_SOCK='+sock+'\n') if sock else '# No SSH_AUTH_SOCK in prior daemon environment.\n')
    shutil.copy2(__file__, dest / 'migrate.py')
    plan = {'preparedAt':datetime.now(timezone.utc).isoformat(), 'binarySha256':EXPECTED_BINARY,
            'sourceHashes':{str(p):sha(p) for p in [source,*drops]},
            'files':{p.name:sha(p) for p in dest.iterdir() if p.is_file()}}
    (dest / 'plan.json').write_text(json.dumps(plan, indent=2)+'\n')
    for p in dest.iterdir(): p.chmod(0o400)
    print('Prepared only; no service or privileged changes:',dest)
    print('plan.json sha256:',sha(dest/'plan.json'))

def apply(dest):
    assert os.geteuid() == 0 and os.environ.get('SUDO_USER') == 'dracon', 'operator sudo required'
    assert pwd.getpwnam('dracon').pw_uid == UID and pwd.getpwnam('dracon').pw_gid == GID
    plan = json.loads((dest/'plan.json').read_text())
    for name, digest in plan['files'].items():
        assert '/' not in name and name not in ('.','..')
        assert not (dest/name).is_symlink() and sha(dest/name) == digest, 'frozen plan changed'
    for path,digest in plan['sourceHashes'].items():
        assert not Path(path).is_symlink() and sha(path)==digest, 'user unit drift'
    assert sha(BIN) == plan['binarySha256'] == EXPECTED_BINARY
    assert userctl('is-active','dracon-sync.service').stdout.strip() == 'active'
    assert userctl('is-enabled','dracon-sync.service').stdout.strip() == 'enabled'
    watchdog_enabled = userctl('is-enabled','dracon-sync-watchdog.timer',check=False).stdout.strip() == 'enabled'
    assert SYSTEM.is_dir(), 'system runtime directory missing: '+str(SYSTEM)
    assert os.access(SYSTEM, os.W_OK), 'system runtime directory not writable as root (are you root?): '+str(SYSTEM)
    assert not any((SYSTEM/n).exists() or (SYSTEM/n).is_symlink() for n in
                   [UNIT,UNIT+'.d',WATCH+'.service',WATCH+'.timer','dracon-sync-operator.environment'])
    assert not any(p.exists() for p in freeze_markers()), 'pre-existing freeze must be resolved'
    hold = HOME/'.dracon/dracon-sync.maintenance-hold'
    assert not hold.exists() and not hold.is_symlink(), 'pre-existing maintenance hold'
    backup = dest/'handover-backup'
    backup.mkdir(mode=0o700)
    shutil.copy2(CONFIG/'dracon-sync.service',backup/'dracon-sync.service')
    # Validate generated units offline before any scheduler is stopped,
    # including the exact installed composition (unit + .d drop-ins).
    run([TOOLS+'/systemd-analyze','verify',dest/UNIT,dest/(WATCH+'.service'),dest/(WATCH+'.timer')])
    verify_tree = Path(tempfile.mkdtemp(prefix='sync-migration-verify-'))
    try:
        shutil.copyfile(dest/UNIT, verify_tree/UNIT)
        verify_drop = verify_tree/(UNIT+'.d');verify_drop.mkdir()
        for name in ['10-resource-headroom.conf','20-bridge-fixture.conf']:
            shutil.copyfile(dest/name, verify_drop/name)
        run([TOOLS+'/systemd-analyze','verify',UNIT], extra_env={'SYSTEMD_UNIT_PATH':str(verify_tree)})
    finally:
        shutil.rmtree(verify_tree, ignore_errors=True)
    hold.touch(mode=0o600,exist_ok=False);os.chown(hold,UID,GID)
    installed=[]; moved=False; stopped=False
    try:
        user(str(BIN),'pause')
        userctl('disable','--now','dracon-sync-watchdog.timer')
        userctl('stop','dracon-sync-watchdog.service')
        require_stopped(userctl, 'dracon-sync-watchdog.service')
        # Explicit operator-approved migration, protected by hold+freeze+rollback.
        stopped=True
        userctl('disable','--now','dracon-sync.service')
        require_stopped(userctl, 'dracon-sync.service')
        shutil.move(CONFIG/'dracon-sync.service',backup/'original.service');moved=True
        (CONFIG/'dracon-sync.service').symlink_to('/dev/null')
        userctl('daemon-reload')
        for name in [UNIT,WATCH+'.service',WATCH+'.timer','dracon-sync-operator.environment']:
            target=SYSTEM/name
            shutil.copyfile(dest/name,target);os.chmod(target,0o600 if name.endswith('.environment') else 0o644);installed.append(target)
        drop=SYSTEM/(UNIT+'.d');drop.mkdir();installed.append(drop)
        for name in ['10-resource-headroom.conf','20-bridge-fixture.conf']:
            shutil.copyfile(dest/name,drop/name);os.chmod(drop/name,0o644)
        run([TOOLS+'/systemctl','daemon-reload'])
        try:
            run([TOOLS+'/systemctl','enable','--now',UNIT],seconds=90)
        except subprocess.CalledProcessError as start_error:
            dump_failure(dest, 'enable-now-'+UNIT, start_error)
            raise
        assert run([TOOLS+'/systemctl','is-active',UNIT]).stdout.strip()=='active'
        pid=int(run([TOOLS+'/systemctl','show',UNIT,'-p','MainPID','--value']).stdout.strip())
        status=Path('/proc/'+str(pid)+'/status').read_text()
        assert next(l for l in status.splitlines() if l.startswith('Uid:')).split()[1:] == [str(UID)]*4
        assert sha('/proc/'+str(pid)+'/exe') == EXPECTED_BINARY
        assert userctl('is-enabled','dracon-sync.service',check=False).stdout.strip()=='masked'
        run([TOOLS+'/systemctl','enable','--now',WATCH+'.timer'])
        user(str(BIN),'resume')
        assert not any(p.exists() for p in freeze_markers()), 'resume left a freeze marker'
        hold.unlink()
        receipt={'at':datetime.now(timezone.utc).isoformat(),'service':UNIT,'pid':pid,'uid':UID,
                 'binarySha256':EXPECTED_BINARY,'installPath':str(SYSTEM),
                 'runtimeInstallEphemeral':True,'userSchedulerMasked':True,'systemWatchdogEnabled':True,
                 'freezeCleared':not (HOME/'.dracon/dracon-sync.freeze').exists(),'holdCleared':not hold.exists(),
                 'rollback':str(backup),'priorWatchdogEnabled':watchdog_enabled}
        (dest/'migration-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
        os.chown(dest/'migration-receipt.json',UID,GID)
        print(json.dumps(receipt))
    except BaseException as original_error:
        # Re-establish the hold even if failure happened after the successful
        # path removed it. Fail closed before resurrecting the old scheduler.
        hold.touch(mode=0o600, exist_ok=True)
        os.chown(hold, UID, GID)
        try:
            if installed:
                systemctl('daemon-reload')
            if (SYSTEM/(WATCH+'.timer')).exists():
                systemctl('disable', '--now', WATCH+'.timer')
                require_stopped(systemctl, WATCH+'.timer')
            if (SYSTEM/(WATCH+'.service')).exists():
                systemctl('stop', WATCH+'.service')
                require_stopped(systemctl, WATCH+'.service')
            if (SYSTEM/UNIT).exists():
                run([TOOLS+'/systemctl','disable','--now',UNIT], seconds=90)
                require_stopped(systemctl, UNIT)
            # Do not remove unit files or restart user scheduling until every
            # possible system watchdog/scheduler process has stopped.
            for p in reversed(installed):
                if p.is_dir(): shutil.rmtree(p)
                elif p.exists(): p.unlink()
            systemctl('daemon-reload')
            if moved:
                (CONFIG/'dracon-sync.service').unlink(missing_ok=True)
                shutil.move(backup/'original.service',CONFIG/'dracon-sync.service')
            userctl('daemon-reload')
            if stopped: userctl('enable','--now','dracon-sync.service')
            if watchdog_enabled: userctl('enable','--now','dracon-sync-watchdog.timer')
            user(str(BIN),'resume')
            assert not any(p.exists() for p in freeze_markers()), 'rollback resume left a freeze marker'
            assert userctl('is-active','dracon-sync.service').stdout.strip()=='active', 'rollback daemon not active'
            hold.unlink()
        except BaseException as recovery_error:
            raise RuntimeError('Rollback incomplete; maintenance hold retained at '+str(hold)+
                               '. Do not start another scheduler. Inspect system scheduler/watchdog state and backup '+
                               str(backup)+' before recovery. Failure: '+type(recovery_error).__name__) from original_error
        raise

if __name__ == '__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('mode',choices=['prepare','apply']);ap.add_argument('directory',type=Path)
    a=ap.parse_args()
    try:
        (prepare if a.mode=='prepare' else apply)(a.directory)
    except Exception as error:
        print('Migration refused/failed:',type(error).__name__,str(error),file=sys.stderr)
        sys.exit(1)
