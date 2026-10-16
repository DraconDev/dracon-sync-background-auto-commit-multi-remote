/** Trusted-bridge protocol only. Repository JSON never selects executable code or transport. */
import { constants } from 'node:fs';
import { lstat, open, link, unlink } from 'node:fs/promises';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { loadProfile, verifyInstalledRunner, validateBinding } from './capabilities.mjs';
import { bindRepository } from './bindings.mjs';
import { inspectConsumer, MANIFEST } from './consumer.mjs';
import { planRepository, publishRepository } from './publisher.mjs';
import { restoreRepository } from './restore.mjs';
import { createPrivateS3Transport } from './storage.mjs';
import { validateBudget } from './routing.mjs';

export const LIMIT = 2 * 1024 * 1024;
export const DESCRIPTOR = 'bucket-binding.json';
const canonical = value => `${JSON.stringify(value, null, 2)}\n`;
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const require = ok => { if (!ok) throw new Error('refused'); };
const exact = (o, keys) => o && typeof o === 'object' && !Array.isArray(o)
  && Object.keys(o).sort().join(',') === [...keys].sort().join(',');
const absolute = p => typeof p === 'string' && p.length <= 4096 && isAbsolute(p) && resolve(p) === p
  && p !== '/' && !/[\\\x00-\x1f\x7f]/.test(p);
const safe = p => typeof p === 'string' && p.length <= 4096 && p.length > 0 && !isAbsolute(p)
  && !/[\\:\x00-\x1f\x7f]/.test(p) && p.split('/').every(s => s && s !== '.' && s !== '..');
const refused = reason => ({ schemaVersion: 1, status: 'refused', reason, commitEligible: false });
export const VERSION = Object.freeze({ schemaVersion: 1, protocolVersion: 1, runner: 'bucket-runner', operations: ['inspect', 'publish', 'restore'] });

export function validateRequest(r) {
  const fields = ['schemaVersion', 'operation', 'root', 'profilePath'];
  require(r && ['inspect', 'publish', 'restore'].includes(r.operation));
  if (r.operation === 'publish') fields.push('stateDirectory', 'remoteName', 'trustedRemoteUrl', 'budget', 'allowedCandidates');
  require(exact(r, fields) && r.schemaVersion === 1 && absolute(r.root) && absolute(r.profilePath));
  if (r.operation === 'publish') {
    require(absolute(r.stateDirectory) && typeof r.remoteName === 'string' && /^[A-Za-z0-9_-]{1,80}$/.test(r.remoteName)
      && typeof r.trustedRemoteUrl === 'string' && r.trustedRemoteUrl.length > 0 && r.trustedRemoteUrl.length <= 4096
      && !/^[\-]|[\s\x00-\x1f\x7f]/.test(r.trustedRemoteUrl));
    require(exact(r.budget, ['projectedBytes', 'limitBytes', 'largeBytes', 'pressureRatio']));
    validateBudget(r.budget);
    require(Array.isArray(r.allowedCandidates) && r.allowedCandidates.length <= 20000
      && r.allowedCandidates.every(safe) && new Set(r.allowedCandidates).size === r.allowedCandidates.length);
  }
  return r;
}
const stamp = s => [s.dev, s.ino, s.mode, s.uid, s.nlink, s.size, s.mtimeNs, s.ctimeNs].join(':');
async function directory(path, privateLeaf = false) {
  require(absolute(path));
  for (let p = path; ; p = dirname(p)) {
    const s = await lstat(p, { bigint: true });
    require(s.isDirectory() && !s.isSymbolicLink() && (s.uid === 0n || s.uid === BigInt(process.geteuid()))
      && (!(s.mode & 0o022n) || (p !== path && s.uid === 0n && (s.mode & 0o1000n))));
    if (p === path) require(s.uid === BigInt(process.geteuid()) && (!privateLeaf || (s.mode & 0o777n) === 0o700n));
    if (p === dirname(p)) break;
  }
}
async function read(path, cap, missing = false) {
  await directory(dirname(path));
  let before;
  try { before = await lstat(path, { bigint: true }); }
  catch (e) { if (missing && e.code === 'ENOENT') return null; throw e; }
  require(before.isFile() && !before.isSymbolicLink() && before.uid === BigInt(process.geteuid())
    && !(before.mode & 0o022n) && before.nlink === 1n && before.size <= BigInt(cap));
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    require(stamp(before) === stamp(await file.stat({ bigint: true })));
    const bytes = Buffer.alloc(Number(before.size) + 1);
    let n = 0;
    while (n < bytes.length) { const r = await file.read(bytes, n, bytes.length - n, n); if (!r.bytesRead) break; n += r.bytesRead; }
    require(n === Number(before.size) && stamp(before) === stamp(await file.stat({ bigint: true }))
      && stamp(before) === stamp(await lstat(path, { bigint: true })));
    return bytes.subarray(0, n);
  } finally { await file.close(); }
}
async function jsonFile(path, missing = false, cap = 256 * 1024) {
  const bytes = await read(path, cap, missing);
  if (bytes === null) return null;
  const value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  require(bytes.equals(Buffer.from(canonical(value))));
  return { value, bytes };
}
async function descriptor(root, binding) {
  const path = join(root, DESCRIPTOR), bytes = Buffer.from(canonical(binding));
  const prior = await read(path, 4096, true);
  if (prior) { require(prior.equals(bytes)); return; }
  const temp = join(root, `.binding-${randomUUID()}.tmp`);
  const file = await open(temp, constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW, 0o644);
  try {
    try { await file.writeFile(bytes); await file.sync(); } finally { await file.close(); }
    try { await link(temp, path); }
    catch (e) { if (e.code !== 'EEXIST') throw e; }
  } finally { await unlink(temp); }
  require((await read(path, 4096)).equals(bytes));
}
function coherent(manifest, binding) {
  require(manifest?.schemaVersion === 1 && manifest.bucket === 'dracon-lfs' && manifest.repository === binding.repositoryId
    && manifest.namespace === binding.namespace && Array.isArray(manifest.assets) && manifest.assets.length <= 20000);
}
function consumerVerdict(c) {
  // Reasons originate in static qualification; never return plan bytes or arbitrary exception messages.
  const reason = /^[a-zA-Z0-9:._-]{1,160}$/.test(c.reason) ? c.reason : 'consumer-unsupported';
  return { schemaVersion: 1, status: c.status, reason, commitEligible: false, runtimeQualified: true };
}

/** Only a library caller can replace storage, never serialized requests. All other gates remain real. */
export function createRunner({ transportFactory = createPrivateS3Transport, executingPath = fileURLToPath(import.meta.url) } = {}) {
  return async input => {
    let transport;
    try {
      const r = validateRequest(input);
      const profile = await loadProfile(r.profilePath);
      require(profile.enabled && profile.runner.path === executingPath && executingPath.endsWith('/runner.bundle.mjs'));
      await verifyInstalledRunner(profile);
      await directory(r.root);
      const initial = await inspectConsumer({ root: r.root });
      if (r.operation === 'inspect' || initial.status !== 'supported') return consumerVerdict(initial);
      const portable = await jsonFile(join(r.root, DESCRIPTOR), true, 4096);
      let binding;
      const manifestPath = join(r.root, MANIFEST);
      const prior = await jsonFile(manifestPath, true);
      if (r.operation === 'restore') {
        require(portable && prior);
        binding = validateBinding(profile, portable.value);
        coherent(prior.value, binding);
        transport = await transportFactory(profile, binding, { role: 'consumer' });
        const result = await restoreRepository({ root: r.root, profile, binding, manifest: prior.value, transport });
        return { schemaVersion: 1, status: 'restored', commitEligible: false, manifestSha256: sha(prior.bytes),
          materializedPaths: result.outcomes.map(o => o.sourcePath), outcomes: result.outcomes,
          generatedInternalPaths: ['.bucket-cache'] };
      }
      await directory(r.stateDirectory, true);
      binding = (await bindRepository({ stateDirectory: r.stateDirectory, profile, repositoryRoot: r.root,
        remoteName: r.remoteName, trustedRemoteUrl: r.trustedRemoteUrl, ...(portable ? { binding: portable.value } : {}) })).binding;
      // Existing files are validated, never converted or repaired by the CLI.
      if (prior) coherent(prior.value, binding);
      const indexPath = join(r.stateDirectory, `${binding.repositoryId}.index.json`);
      const previousIndex = await jsonFile(indexPath, true);
      if (previousIndex) require(prior && previousIndex.value.manifestDigest === sha(prior.bytes)
        && previousIndex.value.repositoryId === binding.repositoryId && previousIndex.value.namespace === binding.namespace
        && previousIndex.value.profileId === profile.id && previousIndex.value.manifestPath === manifestPath);
      await descriptor(r.root, binding);
      transport = await transportFactory(profile, binding, { role: 'uploader' });
      const result = await publishRepository({ root: r.root, profile, binding, consumer: initial.consumer,
        manifestPath, indexPath, transport, budget: r.budget, allowedCandidates: r.allowedCandidates });
      if (result.status === 'refused' || result.plan?.status !== 'planned') return refused('publication-refused');
      const fresh = await inspectConsumer({ root: r.root });
      if (fresh.status !== 'supported' || fresh.evidence.packageDigest !== initial.evidence.packageDigest) return refused('consumer-changed');
      const manifest = await jsonFile(manifestPath), index = await jsonFile(indexPath);
      coherent(manifest.value, binding);
      require(index.value.schemaVersion === 1 && index.value.profileId === profile.id
        && index.value.repositoryId === binding.repositoryId && index.value.namespace === binding.namespace
        && index.value.manifestPath === manifestPath && index.value.manifestDigest === sha(manifest.bytes));
      require((await read(join(r.root, DESCRIPTOR), 4096)).equals(Buffer.from(canonical(binding))));
      const current = await planRepository({ root: r.root, profile, binding, consumer: fresh.consumer,
        manifest: manifest.value, budget: r.budget, allowedCandidates: r.allowedCandidates });
      require(current.plan.status === 'planned' && current.plan.complete && current.toUpload.length === 0);
      const paths = [];
      for (const entry of manifest.value.assets) {
        require(safe(entry.sourcePath) && r.allowedCandidates.includes(entry.sourcePath)
          && current.plan.outcomes.some(o => o.sourcePath === entry.sourcePath && o.evidence
            && ['eligible', 'already-registered'].includes(o.status)));
        const bytes = await read(join(r.root, entry.sourcePath), 64 * 1024 * 1024);
        require(bytes.length === entry.bytes && sha(bytes) === entry.sha256);
        paths.push(entry.sourcePath);
      }
      const last = await inspectConsumer({ root: r.root });
      require(last.status === 'supported' && last.evidence.snapshotDigest === fresh.evidence.snapshotDigest);
      require((await read(manifestPath, 256 * 1024)).equals(manifest.bytes));
      return { schemaVersion: 1, status: 'published', commitEligible: true, manifestSha256: sha(manifest.bytes),
        portablePaths: [DESCRIPTOR, MANIFEST], materializedPaths: paths,
        outcomes: current.plan.outcomes.map(o => ({ sourcePath: o.sourcePath, status: o.status })),
        generatedInternalPaths: [`${MANIFEST}.lock`, '.bucket-cache'] };
    } catch { return refused('request-or-authority-refused'); }
    finally { if (transport) { try { await transport.close(); } catch {} } }
  };
}

export async function main(args = process.argv.slice(2), stdin = process.stdin) {
  let verdict;
  try {
    if (args.length === 1 && args[0] === '--version') verdict = VERSION;
    else {
      require(args.length === 0);
      const chunks = []; let size = 0;
      for await (const chunk of stdin) { size += Buffer.byteLength(chunk); require(size <= LIMIT); chunks.push(Buffer.from(chunk)); }
      verdict = await createRunner()(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks))));
    }
  } catch { verdict = refused('invalid-request'); }
  let output = JSON.stringify(verdict);
  if (Buffer.byteLength(output) > LIMIT) output = JSON.stringify(refused('verdict-size-limit'));
  process.stdout.write(`${output}\n`);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
