"""Append useful runtime evidence; let the daemon alone commit/publish it."""
import datetime,hashlib,json,os,re,stat,subprocess,time,tomllib
from pathlib import Path
OUT=Path(__file__).resolve().parent
CHECKS=OUT.parent
q=json.loads((OUT/'receipt.json').read_text());deploy=json.loads((OUT/'deployment.json').read_text())
installed=json.loads((OUT/'installed-execution-receipt.json').read_text())
assert q['status']=='passed' and installed['ok'] and installed['binary_sha256']==q['binary_sha256']==deploy['loaded_sha256']
before=json.loads((CHECKS/'refs-final-v6-before-work.json').read_text())
assert before['same_loaded_revision_throughout'] and before['daemon_after']['loaded_sha256']==q['binary_sha256']
assert before['daemon_after']['pid']==deploy['new_pid']
old=json.loads((CHECKS/'final-eligible-work-witnesses.json').read_text())
assert len(old['repositories'])==10
source=json.loads((OUT/'source.json').read_text())
policy=Path(source['repositories']['dracon-sync']['private_checkout'])/'src/policy.rs'
code=policy.read_bytes()
assert hashlib.sha256(code).hexdigest()==q['before']['rust_tests_manifest_inputs']['dracon-sync/src/policy.rs']
commit_budget=int(re.search(rb'fn default_stage_op_timeout_secs\(\) -> u64\s*\{\s*(\d+)',code).group(1))
def overrides(path):
 if not path.exists():return []
 found=[]
 def visit(value):
  if isinstance(value,dict):
   for key,item in value.items():
    if key=='stage_op_timeout_secs':found.append(item)
    visit(item)
  elif isinstance(value,list):
   for item in value:visit(item)
 visit(tomllib.loads(path.read_text()));return found
assert not overrides(Path.home()/'.dracon/utilities/sync/dracon-sync.toml'),'resolve operator stage override before recording a bound'
for previous in old['repositories']:
 assert not overrides(Path(previous['repo'])/'.dracon/dracon-sync.toml'),'resolve repo stage override before recording a bound'
(CHECKS/'commit-event-correlation-budget.json').write_text(json.dumps({'observed_at':time.time(),'loaded_binary_sha256':q['binary_sha256'],'default_source_ref':source['repositories']['dracon-sync']['ref'],'default_source_function':'default_stage_op_timeout_secs','source_verified_against_qualified_input_hash':True,'effective_stage_op_timeout_secs':commit_budget,'operator_and_repo_overrides_absent':True},indent=2)+'\n')
observations={row['repo']:row for row in before['repositories']}
rows=[]
def git(repo,*args,input=None):
 return subprocess.check_output(['git','-C',str(repo),*args],input=input,timeout=30)
for previous in old['repositories']:
 repo=Path(previous['repo']);path=repo/previous['eligible_path']
 if not path.exists():
  renamed=repo/('AUDIT-'+Path(previous['eligible_path']).name)
  assert renamed.is_file(),f'eligible note was removed or moved; preserve that edit: {repo}'
  path=renamed
 assert stat.S_ISREG(path.lstat().st_mode) and not path.is_symlink()
 start=time.time();head=git(repo,'rev-parse','HEAD').decode().strip()
 when=datetime.datetime.now(datetime.timezone.utc).isoformat()
 section=(f'\n## Qualified runtime refresh — {when}\n\n'
          f'Loaded daemon SHA-256: `{q["binary_sha256"]}`.\n'
          'The qualified runtime distinguishes hook/environment failures from policy refusals; '
          'native build pools are bounded without bypassing lifecycle or product gates. '
          'This operational update is eligible work for autonomous commit/publication verification, '
          'not a claim that publication has already happened.\n')
 fd=os.open(path,os.O_WRONLY|os.O_APPEND|os.O_NOFOLLOW)
 try:
  identity=os.fstat(fd);payload=section.encode();assert os.write(fd,payload)==len(payload);os.fsync(fd)
 finally:os.close(fd)
 current=path.stat();assert current.st_dev==identity.st_dev and current.st_ino==identity.st_ino
 content=path.read_bytes();assert content.endswith(section.encode()),'concurrent note edit observed; do not overwrite it'
 row={'repo':str(repo),'eligible_path':str(path.relative_to(repo)),'eligible_blob':git(repo,'hash-object','--stdin',input=content).decode().strip(),
      'content_sha256':hashlib.sha256(content).hexdigest(),'commit_event_budget_secs':commit_budget,'file_mtime':current.st_mtime,'before_head':head,'before_observed_at':start,
      'intended_destinations':observations[str(repo)]['intended_destinations'],'daemon_owned_commit':'not yet established; no manual add/commit/push performed'}
 rows.append(row)
 result={'recorded_at':time.time(),'expected_loaded_sha256':q['binary_sha256'],'before_snapshot':'refs-final-v6-before-work.json','commit_event_budget_evidence':'commit-event-correlation-budget.json','repositories':rows}
 (CHECKS/'final-v6-eligible-work-witnesses.json').write_text(json.dumps(result,indent=2)+'\n')
print('appended eligible runtime evidence in',len(rows),'repositories; no Git mutation command issued')
