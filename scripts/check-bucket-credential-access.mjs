/** Local-only access preflight. Never sends S3 requests or prints credential data. */
import { fileURLToPath } from 'node:url';
import { loadProfile, verifyInstalledRunner } from './bucket-runner/capabilities.mjs';
import { createPrivateS3Transport } from './bucket-runner/storage.mjs';

export async function checkCredentialAccess(profilePath) {
  const result = { schemaVersion: 1, check: 'local-file-access-only',
    roles: { uploader: false, consumer: false }, networkRequests: 0 };
  try {
    const profile = await loadProfile(profilePath);
    if (!profile.enabled) return { ...result, ok: false };
    await verifyInstalledRunner(profile);
    const binding = { schemaVersion: 1, profileId: profile.id,
      repositoryId: 'local-access-check', namespace: `${profile.storage.namespaceRoot}/qualification-access-check` };
    // The real parser enforces file/ancestor ownership, mode, no-follow,
    // bounded stable reads and exact scoped field grammar. Even accidental
    // future SDK requests are intercepted locally, counted and refused.
    const requestHandler = { async handle() {
      result.networkRequests++;
      throw new Error('network forbidden during local access preflight');
    }, destroy() {} };
    for (const role of ['uploader', 'consumer']) {
      let transport;
      try {
        transport = await createPrivateS3Transport(profile, binding, { role, requestHandler });
        result.roles[role] = true;
      } catch { /* Never return SDK/parser exceptions or file contents. */ }
      finally {
        if (transport) { try { await transport.close(); } catch { result.roles[role] = false; } }
      }
    }
  } catch { /* Profile/runner authority failures are also redacted. */ }
  result.ok = result.roles.uploader && result.roles.consumer && result.networkRequests === 0;
  return result;
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4 || process.argv[2] !== '--profile') {
    console.error('usage: check-bucket-credential-access.mjs --profile ABSOLUTE_PROFILE');
    process.exitCode = 2;
  } else {
    try {
      const result = await checkCredentialAccess(process.argv[3]);
      console.log(JSON.stringify(result));
      process.exitCode = result.ok ? 0 : 1;
    } catch {
      console.error('local credential access preflight refused'); process.exitCode = 1;
    }
  }
}
