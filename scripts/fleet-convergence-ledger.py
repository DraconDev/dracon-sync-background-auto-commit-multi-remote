import datetime, hashlib, json, subprocess
from pathlib import Path
from fleet_convergence_witness import select_witness
root=Path('/home/dracon/Dev/dracon-utilities/dracon-sync/evidence/fleet-convergence-2026-10-23/checks')
w=json.loads((root/'final-eligible-work-witnesses.json').read_text())
before=json.loads((root/'refs-final-loaded-before-work.json').read_text())
after=json.loads((root/'refs-final-loaded-after-work.json').read_text())
deploy=json.loads((root/'final-daemon-deployment.json').read_text())
journal=[json.loads(line) for line in (root/'final-loaded-commit-publication-journal.jsonl').read_text().splitlines()]
expected=w['expected_loaded_sha256']
assert expected==deploy['loaded_sha256']
assert all(s['same_loaded_revision_throughout'] and s['daemon_before']['loaded_sha256']==expected and s['daemon_after']['loaded_sha256']==expected for s in [before,after])
by_repo={r['repo']:r for r in after['repositories']}
def git(repo,*args):
 return subprocess.run(['git','-C',repo,*args],capture_output=True,text=True,timeout=30)
def ancestor(repo,a,b):
 return bool(a and b) and git(repo,'merge-base','--is-ancestor',a,b).returncode==0
rows=[]
for witness in w['repositories']:
 repo=witness['repo'];observed=by_repo[repo]
 row={'repo':repo,'eligible_path':witness['eligible_path'],'expected_blob':witness['eligible_blob'],'before_head':witness['before_head'],'destinations':[],'state':'nonhealthy-unproven','external_blocker':None}
 row.update(select_witness(repo,witness,journal,deploy['new_pid'],git,ancestor))
 commit=row['witness_commit']
 if not commit:
  row['reason']='No historical eligible-blob commit satisfying the loaded-daemon checks was found';rows.append(row);continue
 object_ok=row['object_and_advancement_verified'];events=row['matching_loaded_daemon_commit_events']
 all_destinations=True
 for target in observed['intended_destinations']:
  dest={**target,'observations':[],'contains_witness':False,'advanced_from_before':False}
  if target['resolution']=='configured-destination':
   for alias in target['aliases']:
    remote=observed['remotes'][alias]
    prior=next(r for r in before['repositories'] if r['repo']==repo)['remotes'].get(alias,{})
    tip=remote.get('branch_ref');contains=remote['query_succeeded'] and remote.get('object_type')=='commit' and ancestor(repo,commit,tip)
    advanced=contains and bool(prior.get('branch_ref')) and prior['branch_ref']!=tip and ancestor(repo,prior['branch_ref'],tip)
    dest['observations'].append({'alias':alias,'before_ref':prior.get('branch_ref'),'after_ref':tip,'query_succeeded':remote['query_succeeded'],'confirmed_at':remote['confirmed_at'],'contains_witness':contains,'fast_forward_from_before':advanced})
   dest['contains_witness']=any(x['contains_witness'] for x in dest['observations'])
   dest['advanced_from_before']=any(x['fast_forward_from_before'] for x in dest['observations'])
  all_destinations &= dest['contains_witness'] and dest['advanced_from_before']
  row['destinations'].append(dest)
 if object_ok and events and all_destinations:
  row['state']='witnessed-autonomous-publication'
  row['reason']='Exact eligible note blob committed after the final loaded baseline, correlated with that service PID, and advertised descendant refs advanced on all intended destinations. Later edits may remain.'
 else:row['reason']='Missing object, loaded-daemon commit correlation, or intended-destination advancement; not classified healthy or external.'
 rows.append(row)
result={'generated_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'loaded_sha256':expected,'loaded_pid':deploy['new_pid'],'before_snapshot':'refs-final-loaded-before-work.json','after_snapshot':'refs-final-loaded-after-work.json','journal':'final-loaded-commit-publication-journal.jsonl','provenance_limit':'Read-only Git objects and actual advertised refs plus service-PID/time-correlated commit events; not a cryptographic author attestation. Executor made no real-repository add/commit/push calls.','all_initial_repositories_proven':all(r['state']=='witnessed-autonomous-publication' for r in rows),'repositories':rows}
(root/'live-convergence-ledger.json').write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
for r in rows:print(Path(r['repo']).name,r['state'],r['witness_commit'][:10])
