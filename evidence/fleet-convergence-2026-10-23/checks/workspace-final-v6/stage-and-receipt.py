"""Stage a fully qualified artifact and execute only isolated local fixtures."""
import hashlib,json,os,shutil,subprocess
from pathlib import Path
ROOT=Path('/home/dracon/Dev/dracon-utilities')
OUT=Path(__file__).resolve().parent
q=json.loads((OUT/'receipt.json').read_text());assert q['status']=='passed' and q['compiled_input_scope_unchanged']
assert all(row['exit_code']==0 for row in q['commands'])
source=json.loads((OUT/'source.json').read_text());state=Path(source['state_root'])
assert state.parent==Path('/tmp') and state.name.startswith('fleet-workspace-frozen-')
def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
filtered_equivalents=[]
for name,metadata in source['repositories'].items():
 checkout=metadata['private_checkout']
 ref=subprocess.check_output(['git','-C',checkout,'rev-parse','HEAD'],text=True,timeout=30).strip()
 assert ref==metadata['ref'],'frozen source ref changed during qualification'
 differences=subprocess.run(['git','-C',checkout,'diff','--name-only','-z','HEAD','--'],capture_output=True,timeout=180)
 assert differences.returncode==0
 for encoded in differences.stdout.split(b'\0'):
  if not encoded:continue
  path=os.fsdecode(encoded);file=Path(checkout)/path
  assert file.is_file() and not file.is_symlink(),'changed input type requires assessment'
  # Required clean filters may re-encrypt an equivalent smudged fixture.
  # Verify exact ordinary-checkout bytes from the frozen object instead of
  # disabling filters or accepting a different plaintext/source value.
  expected=subprocess.run(['git','-C',checkout,'cat-file','--filters','--path='+path,'HEAD:'+path],capture_output=True,timeout=180)
  assert expected.returncode==0 and file.read_bytes()==expected.stdout,'frozen input differs after required smudge'
  filtered_equivalents.append({'repo':name,'path':path,'required_smudge_byte_equivalence':True})
(OUT/'frozen-input-integrity.json').write_text(json.dumps({'refs_unchanged':True,'tracked_inputs_match_frozen_objects_through_required_filters':True,'filtered_equivalents':filtered_equivalents},indent=2)+'\n')
def identity():
 pid=int(subprocess.check_output(['systemctl','show','dracon-sync-operator.service','-p','MainPID','--value'],text=True,timeout=10))
 return {'pid':pid,'sha256':digest(f'/proc/{pid}/exe')}
before=identity();assert before['sha256']=='8be3718f27c1bc6edee5bb38a27d3911fdb1154715848e148dc13009cb5891cb'
original=Path(q['binary_path']);assert digest(original)==q['binary_sha256']
candidate=state/'qualified-bin/dracon-sync';candidate.parent.mkdir(mode=0o700)
assert not candidate.exists();shutil.copy2(original,candidate);candidate.chmod(0o755);assert digest(candidate)==q['binary_sha256']
dependencies=subprocess.run(['ldd',str(candidate)],capture_output=True,text=True,timeout=20)
(OUT/'runtime-dependencies.log').write_text(dependencies.stdout+dependencies.stderr)
assert '/tmp/fleet-workspace-frozen-' not in dependencies.stdout,'runtime must not depend on disposable build libraries'
execution=state/'candidate-execution';assert not execution.exists()
# The receipt owns finite per-operation waits and a finally block that joins
# its daemon family. Do not SIGKILL that owner with an outer run() timeout.
result=subprocess.run(['python3',str(ROOT/'dracon-sync/scripts/isolated-publication-receipt.py'),str(candidate),str(execution)])
assert result.returncode==0
receipt=json.loads((execution/'receipt.json').read_text());assert receipt['ok'] and receipt['binary_sha256']==q['binary_sha256']
after=identity();assert before==after,'real service identity must remain unchanged during candidate fixtures'
shutil.copyfile(execution/'receipt.json',OUT/'candidate-execution-receipt.json')
(OUT/'candidate-stage.json').write_text(json.dumps({'candidate':str(candidate),'binary_sha256':digest(candidate),'real_service_before':before,'real_service_after':after,'fixture_root_outside_watch_roots':str(execution)},indent=2)+'\n')
