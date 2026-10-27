import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { runFencedMutation, gitEnvironment } from 'file:///home/dracon/Dev/pi-plugins/extensions/pi-canonical-write-fence/client.js';
const exec = promisify(execFile);
// Deployment prerequisite: abort AFTER an ordinary in-place write has begun
// must settle like the native SDK (queue held until the write finishes) and
// must not leave truncated partial bytes. The disposable gate pauses the
// first native write to the ordinary target after open/truncate; the client
// abort (SIGTERM to the owned family) must let that write finish, report
// aborted, preserve full new bytes with the same inode, and release the fence.
test('ordinary in-place write abort settles to full content without truncation and releases fence', { timeout: 30000 }, async t => {
  const warden = process.env.PI_CANONICAL_TEST_WARDEN, sdkUrl = process.env.PI_CANONICAL_TEST_SDK;
  assert.ok(warden && sdkUrl, 'qualified artifacts required');
  const root = await fs.mkdtemp(join(tmpdir(), 'canonical-ordinary-cancel-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const repo = join(root, 'repo');
  await exec('git', ['init', '-q', '--template=', repo], { env: gitEnvironment() });
  const target = join(repo, 'note.txt');
  const originalText = 'original-ordinary-content\n';
  await fs.writeFile(target, originalText, { mode: 0o640 });
  const before = await fs.stat(target, { bigint: true });
  const candidate = 'candidate-ordinary-content-'.repeat(200) + '\n';
  const library = join(root, 'gate-ordinary-write.so');
  await exec('cc', ['-shared', '-fPIC', '-Wall', '-Wextra', '-Werror', '-o', library,
    fileURLToPath(new URL('file:///home/dracon/Dev/pi-plugins/extensions/pi-canonical-write-fence/test/fixtures/gate-ordinary-write.c')), '-ldl'], { timeout: 15000 });
  const ready = join(root, 'gate-ready'), release = join(root, 'gate-release');
  const oldPreload = process.env.LD_PRELOAD;
  const oldTarget = process.env.PI_CANONICAL_GATE_TARGET, oldReady = process.env.PI_CANONICAL_GATE_READY, oldRelease = process.env.PI_CANONICAL_GATE_RELEASE;
  t.after(() => {
    if (oldPreload === undefined) delete process.env.LD_PRELOAD; else process.env.LD_PRELOAD = oldPreload;
    if (oldTarget === undefined) delete process.env.PI_CANONICAL_GATE_TARGET; else process.env.PI_CANONICAL_GATE_TARGET = oldTarget;
    if (oldReady === undefined) delete process.env.PI_CANONICAL_GATE_READY; else process.env.PI_CANONICAL_GATE_READY = oldReady;
    if (oldRelease === undefined) delete process.env.PI_CANONICAL_GATE_RELEASE; else process.env.PI_CANONICAL_GATE_RELEASE = oldRelease;
  });
  process.env.LD_PRELOAD = library + (oldPreload ? ':' + oldPreload : '');
  process.env.PI_CANONICAL_GATE_TARGET = await fs.realpath(target);
  process.env.PI_CANONICAL_GATE_READY = ready;
  process.env.PI_CANONICAL_GATE_RELEASE = release;
  const controller = new AbortController(); t.after(() => controller.abort());
  const run = runFencedMutation({ tool: 'write', params: { path: target, content: candidate }, cwd: repo, target, sdkUrl, warden, signal: controller.signal });
  const result = run.then(value => ({ value }), error => ({ error }));
  for (let i = 0; i < 500; i++) {
    try { await fs.access(ready); break; }
    catch (e) { if (e.code !== 'ENOENT') throw e; }
    await delay(20);
  }
  await fs.access(ready);
  // Open already truncated; the first content write is held inside the gate.
  const during = await fs.readFile(target, 'utf8');
  assert.equal(during, '', 'ordinary open truncates before the gated write; this is the window under test');
  controller.abort();
  // Let the held write settle inside the 5s TERM->KILL grace so the outcome
  // mirrors the native settle-before-release contract instead of a KILL cut.
  await delay(6500); // remain blocked beyond TERM->KILL grace
  await fs.writeFile(release, 'go\n');
  const outcome = await result;
  assert.ok(outcome.error, 'aborted ordinary write must report cancellation');
  assert.match(outcome.error.message, /cancelled|aborted|Operation aborted/i);
  const afterText = await fs.readFile(target, 'utf8');
  assert.equal(afterText, candidate, 'must settle to full new bytes, never truncated partial nor silent original');
  const after = await fs.stat(target, { bigint: true });
  assert.equal(after.ino, before.ino, 'ordinary in-place semantics preserve inode');
  assert.equal(after.mode, before.mode, 'ordinary in-place semantics preserve mode');
  // Fence must be free: a fresh native participant can acquire immediately.
  await exec(warden, ['write-fence', '--repo', repo, '--wait-secs', '1', '--run-secs', '1', '--', 'true'], { timeout: 5000, env: gitEnvironment() });
  // No metadata scratch is used for ordinary writes.
  const scratch = join(repo, '.git/dracon-canonical-writes-v1');
  const entries = await fs.readdir(scratch).catch(e => { if (e.code === 'ENOENT') return null; throw e; });
  if (entries !== null) assert.deepEqual(entries, []);
});
