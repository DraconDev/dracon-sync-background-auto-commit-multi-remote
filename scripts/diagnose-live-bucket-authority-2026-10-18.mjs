/** Status-only, no upload/publish, no manifest or binding creation.
 * Refuses unless existing trusted binding is present. Intended for sandbox probe.
 */
import {readFile} from 'node:fs/promises';
import {inspectConsumer} from './bucket-runner/consumer.mjs';
import {loadProfile,verifyInstalledRunner} from './bucket-runner/capabilities.mjs';
import {bindRepository} from './bucket-runner/bindings.mjs';
const root='/home/dracon/dracon/bridge-fixture';
let stage='profile';
try {
 const p=await loadProfile('/home/dracon/.dracon/state/bucket-runner/profile.json');
 stage='installed-runner';await verifyInstalledRunner(p);
 stage='consumer';const c=await inspectConsumer({root});console.log(JSON.stringify({stage,status:c.status,reason:c.reason}));
 stage='binding';const binding=JSON.parse(await readFile(root+'/bucket-binding.json'));
 const r=await bindRepository({stateDirectory:'/home/dracon/.dracon/state/bucket-runner',profile:p,repositoryRoot:root,
 remoteName:'origin',trustedRemoteUrl:'/home/dracon/.dracon/state/bridge-fixture-remote.git',binding});
 console.log(JSON.stringify({stage,status:r.status}));
}catch(e){console.log(JSON.stringify({stage,error: /^[A-Za-z0-9:._ -]{1,100}$/.test(e.message)?e.message:'redacted error'}));process.exitCode=1;}
