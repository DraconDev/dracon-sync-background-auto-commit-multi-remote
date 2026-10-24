import hashlib, json, os, signal, subprocess, time
from pathlib import Path
ROOT=Path('/home/dracon/Dev/dracon-utilities')
OUT=Path(__file__).resolve().parent
ENV={**os.environ,'CARGO_BUILD_JOBS':'2','RUST_TEST_THREADS':'4'}

def snapshot():
    repos={};inputs={}
    for relative in ['.','dracon-sync','dracon-system','dracon-warden']:
        repo=ROOT/relative
        ref=subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'],text=True,timeout=20).strip()
        repos[relative]=ref
        paths=subprocess.check_output(['git','-C',str(repo),'ls-files','-z'],timeout=20).decode().split('\0')
        for name in paths:
            if not name:continue
            path=repo/name
            if relative=='.':selected=name in ['Cargo.toml','Cargo.lock','.cargo/config.toml']
            else:selected=name.startswith(('src/','tests/','.cargo/')) or name.endswith(('.toml','.lock','.rs'))
            if selected:inputs[str(path.relative_to(ROOT))]=hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    return {'refs':repos,'rust_tests_manifest_inputs':inputs}

manifest={'started':time.time(),'commands':[],'status':'running','before':snapshot(),'environment':{k:ENV[k] for k in ['CARGO_BUILD_JOBS','RUST_TEST_THREADS']}}
active=None

def save():
    (OUT/'receipt.json').write_text(json.dumps(manifest,indent=2)+'\n')

def cancel(number,frame):
    if active is not None and active.poll() is None:
        try:os.killpg(active.pid,signal.SIGKILL)
        except ProcessLookupError:pass
        active.wait(timeout=10)
    manifest['status']='cancelled';save();raise SystemExit(128+number)

signal.signal(signal.SIGTERM,cancel);signal.signal(signal.SIGINT,cancel)
save()
try:
    for name,command,deadline in [
        ('workspace',['cargo','test','--workspace','--locked'],1800),
        ('clippy',['cargo','clippy','--workspace','--locked','--all-targets','--','-D','warnings'],1200),
        ('release',['cargo','build','--release','--locked'],1800),
        ('deny',['cargo','deny','check'],600),
    ]:
        row={'gate':name,'command':command,'started':time.time(),'status':'running','log':str(OUT/(name+'.log'))};manifest['commands'].append(row);save()
        with open(row['log'],'wb') as output:
            active=subprocess.Popen(command,cwd=ROOT,env=ENV,stdout=output,stderr=subprocess.STDOUT,start_new_session=True)
            try:code=active.wait(timeout=deadline)
            except subprocess.TimeoutExpired:
                os.killpg(active.pid,signal.SIGKILL);active.wait(timeout=10);code=124
            active=None
        row.update(exit_code=code,status='passed' if code==0 else 'failed',finished=time.time());save()
    manifest['after']=snapshot()
    manifest['compiled_input_scope_unchanged']=manifest['before']['rust_tests_manifest_inputs']==manifest['after']['rust_tests_manifest_inputs']
    binary=ROOT/'target/release/dracon-sync';manifest['binary_sha256']=hashlib.sha256(binary.read_bytes()).hexdigest()
    manifest['status']='passed' if manifest['compiled_input_scope_unchanged'] and all(row['exit_code']==0 for row in manifest['commands']) else 'failed'
except BaseException as error:
    if manifest['status']!='cancelled':manifest['status']='failed'
    manifest['error']=str(error)
finally:
    manifest['finished']=time.time();save()
raise SystemExit(0 if manifest['status']=='passed' else 1)
