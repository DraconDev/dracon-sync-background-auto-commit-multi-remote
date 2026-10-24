"""Append useful runtime evidence; let the daemon alone commit/publish it."""
import datetime,hashlib,json,os,stat,subprocess,time
from pathlib import Path
OUT=Path(__file__).resolve().parent
CHECKS=OUT.parent
q=json.loads((OUT/'receipt.json').read_text());deploy=json.loads((OUT/'deployment.json').read_text())
installed=json.loads((OUT/'installed-execution-receipt.json').read_text())
assert q['status']=='passed' and installed['ok'] and installed['binary_sha256']==q['binary_sha256']==deploy['loaded_sha256']
before=json.loads((CHECKS/'refs-final-v5-before-work.json').read_text())
assert before['same_loaded_revision_throughout'] and before['daemon_after']['loaded_sha256']==q['binary_sha256']
assert before['daemon_after']['pid']==deploy['new_pid']
old=json.loads((CHECKS/'final-eligible-work-witnesses.json').read_text())
assert len(old['repositories'])==10
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
      'content_sha256':hashlib.sha256(content).hexdigest(),'file_mtime':current.st_mtime,'before_head':head,'before_observed_at':start,
      'intended_destinations':observations[str(repo)]['intended_destinations'],'daemon_owned_commit':'not yet established; no manual add/commit/push performed'}
 rows.append(row)
 result={'recorded_at':time.time(),'expected_loaded_sha256':q['binary_sha256'],'before_snapshot':'refs-final-v5-before-work.json','repositories':rows}
 (CHECKS/'final-v5-eligible-work-witnesses.json').write_text(json.dumps(result,indent=2)+'\n')
print('appended eligible runtime evidence in',len(rows),'repositories; no Git mutation command issued')
