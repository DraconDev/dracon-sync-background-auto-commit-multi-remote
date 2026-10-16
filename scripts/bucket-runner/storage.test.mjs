import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, chmod, symlink, mkdir, rm, stat } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir, homedir } from 'node:os';
import { Readable } from 'node:stream';
import { createPrivateS3Transport, trustedCredentialAncestor } from './storage.mjs';
import { CAPS } from './adapters.mjs';

const key = `games/repo/${'a'.repeat(64)}.png`;
const secret = 'fixture-secret-never-echo';
const credentialText = `OVH_ACCESS_KEY_ID=fixture-access\nOVH_SECRET_ACCESS_KEY=${secret}\n`;
async function fixture(t) {
  // /tmp is intentionally rejected: even sticky writable ancestors aren't trusted.
  // Outside daemon-watched source trees; /tmp is deliberately not credential authority.
  const dir = await mkdtemp(join(homedir(), '.dracon-storage-fixture-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const uploader = join(dir, 'uploader'), consumer = join(dir, 'consumer');
  await writeFile(uploader, credentialText, { mode: 0o600 });
  await writeFile(consumer, credentialText.replace('fixture-access', 'reader-access'), { mode: 0o600 });
  const profile = { schemaVersion: 1, id: 'test', enabled: true,
    runner: { path: '/installed/runner', sha256: '0'.repeat(64), version: 1 },
    storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', region: 'sbg', endpoint: 'https://s3.example.invalid',
      uploaderCredentialFile: uploader, consumerCredentialFile: consumer },
    allowedAssetTrees: ['/fleet/assets'], consumers: [{ id: 'filesystem-restore', version: 1 }] };
  const binding = { schemaVersion: 1, profileId: 'test', repositoryId: 'repo', namespace: 'games/repo' };
  const requests = [];
  let respond = () => ({ statusCode: 200, headers: {}, body: Readable.from([]) });
  let destroyed = false;
  const requestHandler = { async handle(request, options) {
    requests.push({ request, options });
    return { response: await respond(request, options) };
  }, destroy() { destroyed = true; } };
  return { dir, profile, binding, requests, requestHandler,
    respond(fn) { respond = fn; }, get destroyed() { return destroyed; },
    async create(role = 'uploader', timeoutMs = 500) {
      const transport = await createPrivateS3Transport(profile, binding, { role, timeoutMs, requestHandler });
      t.after(() => transport.close());
      return transport;
    } };
}

test('exact SDK requests use selected region, HTTPS endpoint, namespace, credentials and conditional PUT', async t => {
  const f = await fixture(t), transport = await f.create();
  assert.deepEqual(Object.keys(transport).sort(), ['close', 'download', 'head', 'putIfAbsent']);
  assert.equal(await transport.head(key), true);
  const source = join(f.dir, 'snapshot');
  await writeFile(source, 'immutable');
  await transport.putIfAbsent(key, source);
  assert.equal(await readFile(source, 'utf8'), 'immutable');
  const [head, put] = f.requests.map(x => x.request);
  assert.equal(head.method, 'HEAD'); assert.equal(put.method, 'PUT');
  for (const request of [head, put]) {
    assert.equal(request.protocol, 'https:'); assert.equal(request.hostname, 's3.example.invalid');
    assert.equal(request.path, `/dracon-lfs/${key}`);
    assert.match(request.headers.authorization, /Credential=fixture-access\/\d{8}\/sbg\/s3\/aws4_request/);
    assert.ok(!request.headers['x-amz-acl']);
    assert.equal(request.headers['amz-sdk-request'], 'attempt=1; max=1');
  }
  assert.equal(put.headers['if-none-match'], '*');
  assert.equal(put.headers['content-length'], '9');
  assert.equal(Buffer.from(put.body).toString(), 'immutable');
  for (const bad of ['other/' + key, 'games/repo/../escape', key + '?acl']) {
    assert.throws(() => transport.head(bad), /namespace|identity/);
    assert.throws(() => transport.download(bad, source), /namespace|identity/);
    assert.throws(() => transport.putIfAbsent(bad, source), /namespace|identity/);
  }
  assert.equal(f.requests.length, 2);
  transport.close(); assert.equal(f.destroyed, true);
  await assert.rejects(transport.head(key), /closed/);
});

test('consumer has an independent read-only surface and uses only consumer handle', async t => {
  const f = await fixture(t);
  await rm(f.profile.storage.uploaderCredentialFile);
  const transport = await f.create('consumer');
  assert.deepEqual(Object.keys(transport).sort(), ['close', 'download', 'head']);
  assert.equal(transport.putIfAbsent, undefined);
  await transport.head(key);
  assert.match(f.requests[0].request.headers.authorization, /Credential=reader-access\//);
  await assert.rejects(f.create('uploader'), /credential file refused/);
});

test('404 alone means missing; forbidden, outage, redirect and 412 are sanitized, never retried', async t => {
  const f = await fixture(t), transport = await f.create();
  const source = join(f.dir, 'source'); await writeFile(source, 'bytes');
  f.respond(() => ({ statusCode: 404, headers: {}, body: Readable.from([]) }));
  assert.equal(await transport.head(key), false);
  for (const status of [403, 500, 301, 307, 412]) {
    f.respond(() => ({ statusCode: status, headers: { location: 'https://untrusted.invalid/' },
      body: Readable.from([`<Error><Code>Failure</Code><Message>${secret}</Message></Error>`]) }));
    const before = f.requests.length;
    await assert.rejects(status === 412 ? transport.putIfAbsent(key, source) : transport.head(key), error => {
      assert.ok(!error.message.includes(secret)); assert.equal(error.cause, undefined);
      if (status >= 400) assert.equal(error.$metadata.httpStatusCode, status);
      return true;
    });
    assert.equal(f.requests.length, before + 1);
  }
  f.respond(() => { throw new Error(secret); });
  await assert.rejects(transport.head(key), error => !error.message.includes(secret));
});

test('SDK error bodies are bounded before deserialization for downloads and uploads', async t => {
  const f = await fixture(t), transport = await f.create();
  const source = join(f.dir, 'snapshot');
  await writeFile(source, 'qualified snapshot');
  for (const method of ['download', 'putIfAbsent']) {
    let produced = 0, body;
    f.respond(() => ({ statusCode: 403, headers: {}, body: (body = Readable.from((async function* () {
      for (let i = 0; i < 10000; i++) { produced++; yield Buffer.alloc(4096, 120); }
    })())) }));
    await assert.rejects(transport[method](key, source), /private storage/);
    assert.ok(body.destroyed, 'oversized error stream must be destroyed');
    assert.ok(produced <= 20, `collector must stop near 64KiB, consumed ${produced} chunks`);
  }
});

test('credential ancestor authority refuses foreign owners even with mode 0755', () => {
  const directory = uid => ({ uid, mode: 0o40755, isDirectory: () => true, isSymbolicLink: () => false });
  assert.equal(trustedCredentialAncestor(directory(process.geteuid())), true);
  assert.equal(trustedCredentialAncestor(directory(0)), true);
  assert.equal(trustedCredentialAncestor(directory(process.geteuid() + 12345)), false);
});

test('bounded private downloads destroy successful, oversized and failed streams', async t => {
  const f = await fixture(t), transport = await f.create();
  let body;
  const target = join(f.dir, 'download');
  f.respond(() => ({ statusCode: 200, headers: { 'content-length': '5' }, body: (body = Readable.from(['hello'])) }));
  await transport.download(key, target);
  assert.equal(await readFile(target, 'utf8'), 'hello');
  assert.equal((await stat(target)).mode & 0o777, 0o600); assert.ok(body.destroyed);
  for (const advertised of [true, false]) {
    f.respond(() => ({ statusCode: 200, headers: advertised ? { 'content-length': String(CAPS.maxSourceBytes + 1) } : {},
      body: (body = Readable.from([Buffer.alloc(CAPS.maxSourceBytes + 1)])) }));
    await assert.rejects(transport.download(key, target), /private storage/); assert.ok(body.destroyed);
  }
  f.respond(() => ({ statusCode: 200, headers: {}, body: (body = new Readable({ read() { this.destroy(new Error(secret)); } })) }));
  await assert.rejects(transport.download(key, target), error => !error.message.includes(secret));
  assert.ok(body.destroyed);
  f.respond(() => ({ statusCode: 200, headers: {}, body: (body = Readable.from(['x'])) }));
  await chmod(target, 0o644);
  await assert.rejects(transport.download(key, target), /private storage/); assert.ok(body.destroyed);
  await rm(target); await symlink(join(f.dir, 'consumer'), target);
  await assert.rejects(transport.download(key, target), /private storage/); assert.ok(body.destroyed);
});

test('deadlines abort both requests and stalled bodies; close aborts in-flight work', async t => {
  const f = await fixture(t), transport = await f.create('uploader', 30);
  f.respond(() => new Promise(() => {}));
  await assert.rejects(transport.head(key), /private storage/);
  assert.ok(f.requests[0].options.abortSignal.aborted);
  let body;
  f.respond(() => ({ statusCode: 200, headers: {}, body: (body = new Readable({ read() {} })) }));
  await assert.rejects(transport.download(key, join(f.dir, 'target')), /private storage/);
  await new Promise(resolve => setImmediate(resolve));
  assert.ok(body.destroyed);
  f.respond(() => ({ statusCode: 500, headers: {}, body: (body = new Readable({ read() {} })) }));
  await assert.rejects(transport.download(key, join(f.dir, 'error-target')), /private storage/);
  await new Promise(resolve => setImmediate(resolve));
  assert.ok(body.destroyed, 'deadline must also tear down the SDK error collector');
  f.respond(() => new Promise(() => {}));
  const pending = transport.head(key); transport.close();
  await assert.rejects(pending, /private storage/);
});

test('snapshot bounds, regularity and symlinks fail before any PUT', async t => {
  const f = await fixture(t), transport = await f.create();
  const source = join(f.dir, 'source'); await writeFile(source, Buffer.alloc(CAPS.maxSourceBytes + 1));
  await assert.rejects(transport.putIfAbsent(key, source), /private storage/);
  await rm(source); await symlink(f.profile.storage.consumerCredentialFile, source);
  await assert.rejects(transport.putIfAbsent(key, source), /private storage/);
  await assert.rejects(transport.putIfAbsent(key, f.dir), /private storage/);
  assert.equal(f.requests.length, 0);
});

test('credential grammar, privacy, bounds, leaf and ancestor trust fail closed without ambient fallback', async t => {
  const f = await fixture(t), path = f.profile.storage.uploaderCredentialFile;
  for (const contents of ['', 'OVH_ACCESS_KEY_ID=only', credentialText + 'OTHER=x\n', credentialText + 'OVH_ACCESS_KEY_ID=again\n',
    credentialText.replace(secret, '$(echo unsafe)'), credentialText.replace(secret, '"quoted"'), 'x'.repeat(16385)]) {
    await writeFile(path, contents);
    await assert.rejects(f.create(), /credential file refused/);
  }
  await writeFile(path, credentialText); await chmod(path, 0o644);
  await assert.rejects(f.create(), /credential file refused/);
  await chmod(path, 0o600); await rm(path); await symlink(f.profile.storage.consumerCredentialFile, path);
  await assert.rejects(f.create(), /credential file refused/);
  await rm(path); await mkdir(path);
  await assert.rejects(f.create(), /credential file refused/); await rm(path, { recursive: true });
  await writeFile(path, credentialText, { mode: 0o600 });
  await chmod(f.dir, 0o777);
  try { await assert.rejects(f.create(), /credential file refused/); } finally { await chmod(f.dir, 0o700); }
  const alias = join(f.dir, 'alias'); await symlink(f.dir, alias);
  f.profile.storage.uploaderCredentialFile = join(alias, 'uploader');
  await assert.rejects(f.create(), /credential file refused/);
  f.profile.storage.uploaderCredentialFile = join(f.dir, 'missing');
  // Whatever AWS_* variables are present, this missing explicit file must fail.
  await assert.rejects(f.create(), /credential file refused/);
  assert.equal(f.requests.length, 0);
});

test('publisher-style snapshots and private download caches work under a sticky temporary root', async t => {
  const f = await fixture(t), transport = await f.create();
  const dir = await mkdtemp(join(tmpdir(), 'storage-cache-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const source = join(dir, 'snapshot'); await writeFile(source, 'verified', { mode: 0o600 });
  await transport.putIfAbsent(key, source);
  f.respond(() => ({ statusCode: 200, headers: {}, body: Readable.from(['verified']) }));
  await transport.download(key, source);
  assert.equal(await readFile(source, 'utf8'), 'verified');
  f.profile.storage.uploaderCredentialFile = source;
  await assert.rejects(f.create(), /credential file refused/);
});

test('invalid authority, missing explicit role and unbounded timeouts rejected', async t => {
  const f = await fixture(t);
  await assert.rejects(createPrivateS3Transport(f.profile, f.binding), /role required/);
  await assert.rejects(f.create('uploader', 120001), /timeout/);
  await assert.rejects(f.create('uploader', 0), /timeout/);
  f.profile.storage.endpoint = 'http://insecure.invalid';
  await assert.rejects(f.create(), /HTTPS/);
});
