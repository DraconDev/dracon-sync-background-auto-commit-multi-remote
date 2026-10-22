import hashlib, json, os
from pathlib import Path
import shutil, signal, subprocess, sys, time

artifact = Path('target/release/dracon-sync').resolve()
destination = Path.home()/'.local/bin/dracon-sync'
expected = sys.argv[1]
unit = 'dracon-sync-operator.service'
def digest(p):
    h=hashlib.sha256()
    with open(p,'rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''): h.update(b)
    return h.hexdigest()
def pid():
    return int(subprocess.check_output(['systemctl','show',unit,'-p','MainPID','--value'],text=True,timeout=5).strip())
assert digest(artifact)==expected
assert destination.is_file() and not destination.is_symlink()
old=pid(); assert old>0
owned_pid = os.pidfd_open(old)
assert Path('/proc',str(old)).stat().st_uid==os.getuid()
assert b'dracon-sync\x00daemon' in Path('/proc',str(old),'cmdline').read_bytes()
backup=Path.home()/'.local/state/dracon-sync/deployments'/time.strftime('%Y%m%dT%H%M%SZ-repos-reliability',time.gmtime())
backup.mkdir(mode=0o700,parents=True)
shutil.copy2(destination,backup/'dracon-sync.before')
shutil.copyfile(Path('/proc',str(old),'exe'),backup/'dracon-sync.loaded-before')
receipt={'started_at':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'old_pid':old,'expected_sha256':expected,'previous_sha256':digest(destination),'backup':str(backup),'method':'maintenance, atomic replacement, SIGTERM, Restart=always'}
# Give already finishing work a brief chance to settle, then rely on owned
# graceful cancellation rather than waiting indefinitely on remote gates.
time.sleep(3)
temporary=destination.with_name('.dracon-sync.repos-reliability-new')
assert not temporary.exists()
try:
    shutil.copyfile(artifact,temporary); temporary.chmod(0o755)
    assert digest(temporary)==expected
    os.replace(temporary,destination)
    assert pid() == old, 'daemon restarted during deployment preparation'
    signal.pidfd_send_signal(owned_pid, signal.SIGTERM)
    deadline=time.monotonic()+140
    while time.monotonic()<deadline:
        new=pid()
        if new>0 and new!=old:
            try:
                if digest(Path('/proc',str(new),'exe'))==expected:
                    receipt.update(new_pid=new,running_sha256=expected,finished_at=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()))
                    (backup/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
                    print(json.dumps(receipt,indent=2));break
            except FileNotFoundError: pass
        time.sleep(1)
    else:
        raise RuntimeError('new loaded executable not observed within 140 seconds; backup retained')
finally:
    os.close(owned_pid)
    if temporary.exists(): temporary.unlink()
