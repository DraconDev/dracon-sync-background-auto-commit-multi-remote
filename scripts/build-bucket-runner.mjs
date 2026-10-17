import { createRequire, builtinModules } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { readFile, writeFile } from 'node:fs/promises';
const root = dirname(fileURLToPath(import.meta.url));
const require = createRequire(join(root, 'bucket-runner/package.json'));
const esbuild = require('esbuild');
if (esbuild.version !== '0.25.12') throw new Error('pinned esbuild required');
const output = join(root, 'bucket-runner/runner.bundle.mjs');
const builtins = new Set(builtinModules.flatMap(n => [n, `node:${n}`]));
const result = await esbuild.build({ absWorkingDir: dirname(root), entryPoints: [join(root, 'bucket-runner/runner.mjs')], outfile: output,
  bundle: true, platform: 'node', format: 'esm', target: 'node20', write: false, metafile: true,
  legalComments: 'inline', banner: { js: "import { createRequire as __bundleCreateRequire } from 'node:module'; const require = __bundleCreateRequire(import.meta.url);" } });
for (const entry of Object.values(result.metafile.outputs)) for (const dependency of entry.imports) {
  if (dependency.external && !builtins.has(dependency.path)) throw new Error('nonbuiltin external dependency');
}
for (const input of Object.keys(result.metafile.inputs)) if (/\.test\.|fixtures?\//.test(input)) throw new Error('test input in bundle');
const bytes = result.outputFiles[0].contents;
if (bytes.length >= 16 * 1024 * 1024) throw new Error('bundle exceeds limit');
if (process.argv.slice(2).join(' ') === '--check') {
  if (!Buffer.from(bytes).equals(await readFile(output))) throw new Error('bundle differs');
} else {
  if (process.argv.length !== 2) throw new Error('invalid build arguments');
  await writeFile(output, bytes);
}
console.log(`bucket runner dependency closure verified (${bytes.length} bytes)`);
