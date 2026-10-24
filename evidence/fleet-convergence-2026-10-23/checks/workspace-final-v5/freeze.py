"""Freeze ordinary Git checkouts outside watch roots; never mutate canonical repos."""
import hashlib, json, os, subprocess, tempfile, time
from pathlib import Path
os.umask(0o077)
ROOT=Path('/home/dracon/Dev/dracon-utilities')
OUT=Path(__file__).resolve().parent
STATE=Path(tempfile.mkdtemp(prefix='fleet-workspace-frozen-'))
TREE=STATE/'source'
manifest={'started':time.time(),'state_root':str(STATE),'frozen_root':str(TREE),'repositories':{},'status':'running',
          'operations':'read canonical objects/config; private bare refs and private checkouts only; no real-repository add/commit/push/config mutation'}
def save(): (OUT/'source.json').write_text(json.dumps(manifest,indent=2)+'\n')
def git(repo,*args):
    result=subprocess.run(['git','-C',str(repo),*args],capture_output=True,timeout=180)
    if result.returncode:raise RuntimeError(f'Git {args[0]} failed ({result.returncode}): '+result.stderr.decode(errors='replace')[-1000:])
    return result.stdout
try:
    for relative in ['.','dracon-sync','dracon-system','dracon-warden']:
        source=ROOT/relative
        ref=git(source,'rev-parse','HEAD').decode().strip()
        if relative=='dracon-sync':assert b'local pre-push hook failed (inspect retained hook output' in git(source,'show',ref+':src/git/push.rs')
        if relative=='dracon-system':assert b'sizes.sort_by_key(|b| std::cmp::Reverse(b.1));' in git(source,'show',ref+':src/main.rs')
        algorithm=git(source,'rev-parse','--show-object-format').decode().strip()
        objects=git(source,'rev-parse','--path-format=absolute','--git-path','objects').decode().strip()
        name='parent' if relative=='.' else relative
        bare=STATE/'bare'/(name+'.git');bare.parent.mkdir(parents=True,exist_ok=True)
        subprocess.run(['git','init','--quiet','--bare','--object-format='+algorithm,str(bare)],check=True,timeout=30)
        (bare/'objects/info/alternates').write_text(objects+'\n')
        git(bare,'update-ref','refs/heads/main',ref)
        git(bare,'symbolic-ref','HEAD','refs/heads/main')
        destination=TREE if relative=='.' else TREE/relative
        subprocess.run(['git','clone','--shared','--no-checkout','--quiet',str(bare),str(destination)],check=True,timeout=180)
        config=subprocess.run(['git','-C',str(source),'config','--local','--null','--get-regexp',r'^filter\.dracon\.'],capture_output=True,timeout=30)
        if config.returncode not in [0,1]:raise RuntimeError('could not inspect source filter configuration')
        keys=[]
        for item in config.stdout.split(b'\0'):
            if not item:continue
            key,value=item.split(b'\n',1);key=key.decode();value=value.decode()
            git(destination,'config','--local','--add',key,value);keys.append(key)
        git(destination,'read-tree',ref)
        git(destination,'checkout-index','--all')
        assert git(destination,'rev-parse','HEAD').decode().strip()==ref
        manifest['repositories'][relative]={'source':str(source),'ref':ref,'private_checkout':str(destination),'copied_existing_filter_keys':keys,
                                           'private_origin':str(bare),'canonical_objects_read_only':objects}
        save()
    manifest['status']='passed'
except BaseException as error:
    manifest['status']='failed';manifest['error']=str(error);raise
finally:
    manifest['finished']=time.time();save()
print(json.dumps({'frozen_root':str(TREE),'refs':{key:value['ref'] for key,value in manifest['repositories'].items()}}))
