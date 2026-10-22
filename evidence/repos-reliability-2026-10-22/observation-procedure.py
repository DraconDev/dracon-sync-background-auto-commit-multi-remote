from pathlib import Path
import hashlib,json,subprocess,time,sys
out=Path(sys.argv[1]);out.mkdir(exist_ok=True)
for sample in range(3):
    if sample: time.sleep(150)
    started=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
    r=subprocess.run(['dracon-sync','repos','--json'],capture_output=True,text=True,timeout=180)
    (out/f'{sample}-repos.json').write_text(r.stdout)
    (out/f'{sample}-repos.stderr').write_text(r.stderr)
    unit=subprocess.check_output(['systemctl','show','dracon-sync-operator.service','-p','MainPID','-p','ActiveState','-p','TasksMax'],text=True,timeout=10)
    pid=int(next(l.split('=')[1] for l in unit.splitlines() if l.startswith('MainPID=')))
    h=hashlib.sha256()
    with open(f'/proc/{pid}/exe','rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
    evidence=dict(started_at=started,finished_at=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),repos_exit=r.returncode,unit=unit,loaded_sha256=h.hexdigest(),freeze_present=(Path.home()/'.dracon/dracon-sync.freeze').exists())
    (out/f'{sample}-runtime.json').write_text(json.dumps(evidence,indent=2)+'\n')
    print(json.dumps(evidence),flush=True)
r=subprocess.run(['journalctl','-u','dracon-sync-operator.service','--since',sys.argv[2],'--no-pager'],capture_output=True,text=True,timeout=30)
(out/'journal.txt').write_text(r.stdout)
