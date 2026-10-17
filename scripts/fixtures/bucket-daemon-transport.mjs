/** Hermetic qualification fixture: only storage I/O is substituted. Never installed.
 * The Rust daemon worker invokes the real createRunner/publisher in a child process.
 * This file is NOT collected as a test and never invokes a test runner.
 */
import { createHash } from 'node:crypto';
import { mkdir, writeFile, readFile, appendFile, access, copyFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRunner, VERSION, LIMIT } from '../bucket-runner/runner.mjs';
import { renderSolidPng } from '../bucket-runner/adapters.mjs';
import { PREBUILD, inspectConsumer } from '../bucket-runner/consumer.mjs';
const canonical = value => `${JSON.stringify(value, null, 2)}\n`;
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const exists = async p => { try { await access(p); return true; } catch (e) { if (e.code === 'ENOENT') return false; throw e; } };
export const CONCURRENT = canonical({ independentWriter: 'must survive qualification refusal' });

export async function serve(executingPath, base) {
  if (process.argv.slice(2).join(' ') === '--version') { console.log(JSON.stringify(VERSION)); return; }
  const store = join(base, 'objects'), log = join(base, 'operations.jsonl');
  const controlPath = join(base, 'control.json');
  const requestChunks = []; let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length; if (size > LIMIT) throw new Error('fixture request bound');
    requestChunks.push(chunk);
  }
  const request = JSON.parse(Buffer.concat(requestChunks).toString('utf8'));
  const control = JSON.parse(await readFile(controlPath, 'utf8'));
  const record = row => appendFile(log, `${JSON.stringify(row)}\n`);
  let puts = 0;
  const run = createRunner({ executingPath, transportFactory: async (_profile, _binding, { role }) => {
    const path = key => join(store, sha(key));
    const transport = {
      async head(key) {
        await record({ op: 'head', key, role });
        if (control.mode === 'outage') throw new Error('fixture unavailable');
        return exists(path(key));
      },
      async download(key, target) {
        await record({ op: 'download', key, role });
        await copyFile(path(key), target);
        if (control.mode === 'race' && !control.fired) {
          control.fired = true;
          await writeFile(controlPath, canonical(control));
          await writeFile(join(request.root, 'bucket-assets.manifest.json'), CONCURRENT);
        }
      },
      async close() { await record({ op: 'close', role }); }
    };
    if (role === 'uploader') transport.putIfAbsent = async (key, source) => {
      await record({ op: 'put', key, role });
      if (control.mode === 'partial' && ++puts === 2) throw new Error('fixture interrupted second upload');
      try { await writeFile(path(key), await readFile(source), { flag: 'wx' }); }
      catch (e) { if (e.code !== 'EEXIST') throw e; const conflict = new Error('PreconditionFailed'); conflict.$metadata = { httpStatusCode: 412 }; throw conflict; }
      await record({ op: 'stored', key, role });
    };
    return transport;
  } });
  const diagnostic = await inspectConsumer({ root: request.root });
  await record({ op: 'consumer-diagnostic', status: diagnostic.status, reason: diagnostic.reason });
  const verdict = await run(request);
  await record({ op: 'verdict', status: verdict.status, commitEligible: verdict.commitEligible });
  process.stdout.write(`${JSON.stringify(verdict)}\n`);
  if (verdict.status === 'refused') process.exitCode = 1;
}

async function setup(base) {
  const root = join(base, 'repo'), state = join(base, 'state'), bundle = join(base, 'runner.bundle.mjs');
  await mkdir(state, { mode: 0o700 }); await mkdir(join(base, 'objects'));
  await mkdir(join(root, 'public')); await mkdir(join(root, '.dracon'));
  const pkg = { name: 'daemon-qualification', scripts: { build: 'vite build', prebuild: PREBUILD }, devDependencies: { vite: '7.1.2' } };
  await writeFile(join(root, 'package.json'), canonical(pkg));
  await writeFile(join(root, 'package-lock.json'), canonical({ lockfileVersion: 3, packages: { '': { devDependencies: { vite: '7.1.2' } }, 'node_modules/vite': { version: '7.1.2' } } }));
  await writeFile(join(root, 'index.html'), '<html></html>\n');
  await writeFile(join(root, '.dracon/dracon-sync.toml'), 'owned = true\nauto_bump_versions = false\n');
  for (const [name, rgba] of [['a', [20, 40, 60, 255]], ['b', [180, 160, 140, 255]]]) {
    const bytes = renderSolidPng({ width: 512, height: 512, rgba });
    if (bytes.length <= 1024) throw new Error('fixture must cross real shared large-asset threshold');
    await writeFile(join(root, 'public', `${name}.png`), bytes);
  }
  await writeFile(bundle, `import { serve } from ${JSON.stringify(import.meta.url)};\nawait serve(${JSON.stringify(bundle)}, ${JSON.stringify(base)});\n`);
  const profile = { schemaVersion: 1, id: 'hermetic-daemon', enabled: true,
    runner: { path: bundle, sha256: sha(await readFile(bundle)), version: 1 },
    storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', endpoint: 'https://example.invalid', region: 'sbg',
      uploaderCredentialFile: join(base, 'unused-uploader'), consumerCredentialFile: join(base, 'unused-reader') },
    allowedAssetTrees: [base], consumers: [{ id: 'filesystem-restore', version: 1 }] };
  await writeFile(join(base, 'profile.json'), canonical(profile), { mode: 0o600 });
  await writeFile(join(base, 'control.json'), canonical({ mode: 'healthy' }));
  await writeFile(join(root, 'bucket-runner.json'), canonical({ schemaVersion: 1, enabled: true,
    profilePath: join(base, 'profile.json'), stateDirectory: state, remoteName: 'origin',
    trustedRemoteUrl: 'https://example.invalid/operator/qualification.git', assetRoot: '.',
    manifestPath: 'bucket-assets.manifest.json', allowedCandidates: [] }));
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv[2] !== 'setup' || !process.argv[3]) throw new Error('usage: fixture setup BASE');
  await setup(process.argv[3]);
}
