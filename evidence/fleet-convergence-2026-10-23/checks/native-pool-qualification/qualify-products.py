import datetime, hashlib, json, os, shutil, signal, subprocess, tempfile, time
from pathlib import Path
REPO=Path('/home/dracon/Dev/browser-extensions-shared')
OUT=Path(tempfile.mkdtemp(prefix='fleet-browser-products-'))
TREE=OUT/'snapshot';TREE.mkdir()
Path('/tmp/fleet-browser-products-current.json').write_text(json.dumps({'root':str(OUT)})+'\n')
env=dict(os.environ)
for key in ['GIT_DIR','GIT_WORK_TREE','GIT_COMMON_DIR','GIT_INDEX_FILE','GIT_OBJECT_DIRECTORY','GIT_ALTERNATE_OBJECT_DIRECTORIES','GIT_PREFIX']:
    env.pop(key,None)
manifest={'started':time.time(),'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'scope':'coherent source product diagnostics, not a manual push or a whole-hook pass','commands':[],'status':'running'}
limits={key:env.get(key) for key in ['TOKIO_WORKER_THREADS','RAYON_NUM_THREADS','ROLLDOWN_WORKER_THREADS','ROLLDOWN_MAX_BLOCKING_THREADS','GOMAXPROCS']}
cgroup=next(line.split(':',2)[2] for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))
control=Path('/sys/fs/cgroup')/cgroup.lstrip('/')
manifest['resource_environment']=limits
manifest['cgroup_budget']={'path':cgroup,'pids_max':(control/'pids.max').read_text().strip(),'cpu_max':(control/'cpu.max').read_text().strip()}
assert manifest['cgroup_budget']['pids_max']=='512'
quota,period=manifest['cgroup_budget']['cpu_max'].split();assert int(quota)/int(period)==2
active=None
def cancelled(number, frame):
    if active is not None and active.poll() is None:
        try: os.killpg(active.pid, signal.SIGKILL)
        except ProcessLookupError: pass
        active.wait(timeout=10)
    manifest['status']='cancelled'
    raise SystemExit(f'cancelled by signal {number}')
signal.signal(signal.SIGTERM,cancelled)
signal.signal(signal.SIGINT,cancelled)
def receipt(): (OUT/'receipt.json').write_text(json.dumps(manifest,indent=2)+'\n')
def run(args,cwd,log,seconds=180):
    global active
    row={'command':args,'cwd':str(cwd),'started':time.time(),'log':str(OUT/log),'status':'running'}
    manifest['commands'].append(row);receipt()
    with open(OUT/log,'wb') as output:
        child=subprocess.Popen(args,cwd=cwd,env=env,stdout=output,stderr=subprocess.STDOUT,start_new_session=True)
        active=child
        try:
            try: code=child.wait(timeout=seconds)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=10);code=124
        finally: active=None
    row.update(exit_code=code,finished=time.time(),status='passed' if code==0 else 'failed');receipt()
    return code

def git(*args,cwd=REPO):
    return subprocess.check_output(['git',*args],cwd=cwd,env=env,text=True,timeout=30).strip()
try:
    sha=git('rev-parse','HEAD');manifest['revision']=sha
    required=['bun.lock','.gitignore','packages/nightfall/src/components/Card.svelte.d.ts','packages/nightfall/src/components/StatusHero.svelte.d.ts',
      'extensions/standalones/web-time-tracker-limiter/tailwind.config.mjs',
      'extensions/standalones/web-time-tracker-limiter/tests/nightfall-types.test.ts',
      'extensions/standalones/web-time-tracker-limiter/tests/tailwind.test.ts',
      'extensions/standalones/web-time-tracker-limiter/entrypoints/dashboard/components/NavIcon.svelte',
      'extensions/standalones/tabmon/tools/music_provenance.py',
      'extensions/standalones/tabmon/tools/test_music_provenance.py',
      'extensions/standalones/tabmon/tools/fetch-sfx-mmx.py',
      'extensions/standalones/tabmon/tools/fetch-missing.py',
      'extensions/standalones/tabmon/tools/merge-music-receipt.py',
      'extensions/standalones/tabmon/scripts/framework-lint.mjs',
      'extensions/standalones/tabmon/scripts/framework-lint.check.mjs',
      'extensions/standalones/tabmon/eslint.config.js',
      'extensions/standalones/tabmon/package.json',
      'extensions/standalones/web-time-tracker-limiter/package.json',
      'extensions/standalones/web-time-tracker-limiter/tests/dashboard.test.ts',
      'scripts/git-hooks/run-push-gates.mjs', 'scripts/git-hooks/run-push-gates.test.mjs',
      'packages/extension-core/src/byok/catalogue.ts',
      'packages/extension-core/tests/byok/catalogue-convergence.test.ts']
    manifest['required_source_blobs']={}
    for path in required:
        committed=subprocess.check_output(['git','show',sha+':'+path],cwd=REPO,env=env,timeout=30)
        if committed!=(REPO/path).read_bytes():raise RuntimeError('repair not yet coherently committed: '+path)
        manifest['required_source_blobs'][path]=hashlib.sha256(committed).hexdigest()
    receipt()
    git('init','--quiet',cwd=TREE)
    objects=git('rev-parse','--path-format=absolute','--git-path','objects')
    (TREE/'.git/objects/info/alternates').write_text(objects+'\n')
    (TREE/'.git/HEAD').write_text(sha+'\n')
    (TREE/'.git/info').mkdir(exist_ok=True)
    (TREE/'.git/info/attributes').write_text('* -export-ignore -export-subst\n')
    git('read-tree',sha,cwd=TREE)
    paths=subprocess.check_output(['git','ls-tree','-rz','--name-only',sha],cwd=TREE,env=env,timeout=30).decode().split('\0')
    if any(p and (Path(p).is_absolute() or any(c=='..' or c.lower()=='.git' for c in Path(p).parts)) for p in paths):raise RuntimeError('unsafe tracked path')
    with open(OUT/'source.tar','wb') as output:
        subprocess.run(['git','archive','--format=tar',sha],cwd=TREE,env=env,stdout=output,check=True,timeout=300)
    subprocess.run(['tar','-xf',str(OUT/'source.tar'),'-C',str(TREE)],check=True,timeout=180)
    (OUT/'source.tar').unlink()
    for p in TREE.rglob('*'):
        if p.is_symlink() and not p.resolve().is_relative_to(TREE):raise RuntimeError('escaping source symlink')
    BIN=OUT/'toolchain/bin';BIN.mkdir(parents=True)
    pinned=Path('/home/dracon/.local/state/dracon/sync/gates/browser-extensions-shared/npm-cache/_npx/60c3515df86f25b1/node_modules/bun/bin/bun.exe')
    shutil.copy2(pinned,BIN/'bun');(BIN/'bun').chmod(0o755);(BIN/'bunx').symlink_to('bun')
    manifest['bun_sha256']=hashlib.sha256((BIN/'bun').read_bytes()).hexdigest()
    env['PATH']=str(BIN)+os.pathsep+env['PATH'];env['VITEST_MAX_WORKERS']='2'
    env['BUN_INSTALL_CACHE_DIR']=str(OUT/'bun-cache');env['npm_config_cache']=str(OUT/'npm-cache')
    version=subprocess.check_output([str(BIN/'bun'),'--version'],env=env,text=True,timeout=10).strip()
    if version!='1.3.14':raise RuntimeError('wrong pinned Bun')
    manifest['bun_version']=version
    install=[str(BIN/'bun'),'install','--frozen-lockfile','--backend=copyfile','--concurrent-scripts=1','--network-concurrency=8']
    if run(install+['--ignore-scripts'],TREE,'install-base.log',300):raise RuntimeError('base install refused')
    libraries=[]
    for package in sorted((TREE/'packages').glob('*/package.json')):
        p=json.loads(package.read_text())
        if p.get('scripts',{}).get('build') and '/dist/' in json.dumps([p.get('main',''),p.get('module',''),p.get('exports',{})]):libraries.append(str(package.parent.relative_to(TREE)))
    for index,library in enumerate(libraries):
        if run([str(BIN/'bun'),'run','--cwd',library,'build'],TREE,f'dependency-{index}.log',180):raise RuntimeError('dependency build refused: '+library)
    if run(install,TREE,'install-scripted.log',300):raise RuntimeError('scripted install refused')
    for p in TREE.rglob('*'):
        if '.git' not in p.relative_to(TREE).parts and p.is_symlink() and not p.resolve().is_relative_to(TREE):raise RuntimeError('escaping provisioned-source symlink')
    failed=False
    for product in ['web-time-tracker-limiter','tabmon','vidpro-extension']:
        cwd=TREE/'extensions/standalones'/product
        for script in ['typecheck','build','test','lint']:
            code=run(['npm','--workspaces=false','run',script],cwd,f'{product}-{script}.log',300)
            failed |= code!=0
    manifest['status']='failed' if failed else 'passed'
except BaseException as error:
    if manifest['status']!='cancelled': manifest['status']='failed'
    manifest['error']=str(error)
finally:
    manifest['finished']=time.time();receipt()
print(json.dumps({'root':str(OUT),'status':manifest['status'],'revision':manifest.get('revision'),'error':manifest.get('error')},indent=2))
raise SystemExit(0 if manifest['status']=='passed' else 1)
