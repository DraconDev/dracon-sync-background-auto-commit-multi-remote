"""Disposable focused regression, not whole-product qualification."""
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

repo = Path('/home/dracon/Dev/browser-extensions-shared')
relative = 'extensions/collections/toolkit/gemini-toolkit'
source = repo / relative
out = Path(__file__).parent
head = subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip()
for name in ['lib/panel.ts', 'tests/panel.test.ts', 'entrypoints/gemini.content.ts']:
    assert subprocess.check_output(['git', '-C', str(repo), 'show', head + ':' + relative + '/' + name]) == (source / name).read_bytes()
fixture = Path(tempfile.mkdtemp(prefix='fleet-panel-defaults-counterexample-'))
archive = subprocess.check_output(['git', '-C', str(repo), 'archive', head, '--', relative])
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
    tar.extractall(fixture, filter='data')
app = fixture / relative
deps = app / 'node_modules'
deps.mkdir()
for item in (source / 'node_modules').iterdir():
    if not item.name.startswith('.vite'):
        (deps / item.name).symlink_to(item.resolve(), target_is_directory=item.is_dir())
config = app / 'vitest.config.ts'
config.write_text(config.read_text().replace('export default defineConfig({',
    'export default defineConfig({\n  cacheDir: ' + json.dumps(str(fixture / 'private-vite-cache')) + ',', 1))
env = {**os.environ, 'VITEST_MAX_WORKERS': '2', 'TOKIO_WORKER_THREADS': '2',
       'RAYON_NUM_THREADS': '2', 'ROLLDOWN_WORKER_THREADS': '2',
       'ROLLDOWN_MAX_BLOCKING_THREADS': '8', 'GOMAXPROCS': '2'}
setup = subprocess.run(['npm', '--workspaces=false', 'run', 'postinstall'], cwd=app,
                       env=env, capture_output=True, text=True, timeout=120)
(out / 'fixture-preparation.log').write_text(setup.stdout + setup.stderr)
assert setup.returncode == 0, 'normal private WXT preparation failed'
command = ['npm', '--workspaces=false', 'run', 'test', '--', 'tests/panel.test.ts',
           '-t', 'snapshots canonical defaults']
results = []
for mode in ['canonical', 'duplicated-legacy-defaults']:
    if mode != 'canonical':
        old = subprocess.check_output(['git', '-C', str(repo), 'show',
            '73e4deae3cc3e9bdf89f6285868ebdb9fca4847b:' + relative + '/lib/panel.ts'], text=True)
        start = old.index('    settings: {', old.index('let state: PanelState'))
        end = old.index('\n    activeChatId:', start)
        panel = app / 'lib/panel.ts'
        data = panel.read_text()
        block = '    settings: {\n      ...DEFAULT_SETTINGS,\n      features: { ...DEFAULT_SETTINGS.features },\n    },'
        assert data.count(block) == 1
        panel.write_text(data.replace(block, old[start:end], 1))
    result = subprocess.run(command, cwd=app, env=env, capture_output=True, text=True, timeout=120)
    (out / (mode + '.log')).write_text(result.stdout + result.stderr)
    results.append({'mode': mode, 'exit_code': result.returncode})
assert results[0]['exit_code'] == 0 and results[1]['exit_code'] == 1, results
assert 'getPropertyValue' in (out / 'duplicated-legacy-defaults.log').read_text()
receipt = {'revision': head, 'fixture': str(fixture), 'command': command, 'results': results,
           'scope': 'private tracked source and WXT outputs; linked existing dependencies and private Vite cache; focused counterexample only',
           'live_source_not_modified': True, 'qualification_substitute': False}
(out / 'counterexample.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(results))
