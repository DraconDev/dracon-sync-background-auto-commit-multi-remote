/** Fresh single-fixture restoration through production create-only consumer.
 * Uses an in-memory selection from the real daemon manifest, not a hand-seeded
 * publication manifest. Does not modify the original checkout or any manifest.
 * Legacy concrete-key icon/cover entries are explicitly outside this check.
 */
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdtemp, readFile, writeFile, readdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createPrivateS3Transport} from './bucket-runner/storage.mjs';
import {restoreRepository} from './bucket-runner/restore.mjs';
const evidence=fileURLToPath(new URL('../evidence/bucket-live-qual-2026-10-18/',import.meta.url));
const input=JSON.parse(await readFile(join(evidence,'input-20261019.json')));
const profile=JSON.parse(await readFile('/home/dracon/.dracon/state/bucket-runner/profile.json'));
const manifestRaw=await readFile(join(input.root,'bucket-assets.manifest.json'));
const manifest=JSON.parse(manifestRaw);
const binding=JSON.parse(await readFile(join(input.root,'bucket-binding.json')));
const entry=manifest.assets.find(a=>a.sourcePath===input.path);
assert(entry?.bucketed===true);
assert.equal(entry.sha256,input.sha256);
assert.equal(entry.bytes,input.bytes);
const fresh=await mkdtemp(join(tmpdir(),'ovh-fresh-restore-'));
assert.deepEqual(await readdir(fresh),[]);
const transport=await createPrivateS3Transport(profile,binding,{role:'consumer',timeoutMs:20000});
assert.equal(transport.putIfAbsent,undefined);
let downloads=0;
const reader={head:transport.head,download:async(...args)=>{downloads++;return transport.download(...args);}};
try{
  const result=await restoreRepository({root:fresh,profile,binding,
    manifest:{...manifest,assets:[entry]},transport:reader});
  assert.equal(result.ok,true);
  assert.deepEqual(result.outcomes,[{sourcePath:input.path,status:'restored'}]);
  assert.equal(downloads,1);
  const restored=await readFile(join(fresh,input.path));
  assert.deepEqual(restored,await readFile(join(input.root,input.path)));
  assert.equal(createHash('sha256').update(restored).digest('hex'),input.sha256);
  const retry=await restoreRepository({root:fresh,profile,binding,
    manifest:{...manifest,assets:[entry]},transport:reader});
  assert.deepEqual(retry.outcomes,[{sourcePath:input.path,status:'already-present'}]);
  assert.equal(downloads,1);
  assert.deepEqual(await readFile(join(input.root,'bucket-assets.manifest.json')),manifestRaw);
  const receipt={at:new Date().toISOString(),root:fresh,initiallyEmpty:true,originalCacheUsed:false,
    selectedEntry:entry.sourcePath,scope:'new fixture only; in-memory projection of daemon manifest, no manifest file written',
    legacyEntriesTested:false,transport:'production createPrivateS3Transport consumer role',
    objectKey:binding.namespace+'/'+entry.sha256+'.png',downloads,bytes:restored.length,
    sha256:input.sha256,byteIdenticalToSource:true,outcomes:result.outcomes,retryOutcomes:retry.outcomes,
    manifestUnchanged:true};
  await writeFile(join(evidence,'fresh-restoration-20261019.json'),JSON.stringify(receipt,null,2)+'\n');
  console.log(JSON.stringify(receipt));
}finally{await transport.close();}
