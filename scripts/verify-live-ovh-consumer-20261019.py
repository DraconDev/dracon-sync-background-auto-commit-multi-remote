#!/usr/bin/env python3
"""Independent read-only boto3 GET of the new service-enrolled fixture.
No upload, key creation, IAM calls, manifest writes, or secret output.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import tempfile
import boto3
from botocore.config import Config

here = Path(__file__).resolve().parents[1]
evidence = here / 'evidence/bucket-live-qual-2026-10-18'
input_record = json.loads((evidence / 'input-20261019.json').read_text())
root = Path(input_record['root'])
profile = json.loads((Path.home() / '.dracon/state/bucket-runner/profile.json').read_text())
manifest_bytes = (root / 'bucket-assets.manifest.json').read_bytes()
manifest = json.loads(manifest_bytes)
entry = next(a for a in manifest['assets'] if a['sourcePath'] == input_record['path'])
assert entry['bucketed'] is True
assert entry['sha256'] == input_record['sha256'] and entry['bytes'] == input_record['bytes']
assert manifest['bucket'] == 'dracon-lfs'
key = manifest['namespace'] + '/' + entry['sha256'] + '.png'
creds_path = Path(profile['storage']['consumerCredentialFile'])
assert not creds_path.is_symlink() and creds_path.stat().st_uid == os.getuid()
assert creds_path.stat().st_mode & 0o077 == 0
fields = dict(line.split('=', 1) for line in creds_path.read_text().strip().splitlines())
client = boto3.client('s3', endpoint_url=profile['storage']['endpoint'], region_name=profile['storage']['region'],
    aws_access_key_id=fields['OVH_ACCESS_KEY_ID'], aws_secret_access_key=fields['OVH_SECRET_ACCESS_KEY'],
    config=Config(s3={'addressing_style':'path'}, retries={'max_attempts':0}, connect_timeout=10, read_timeout=20))
try:
    response = client.get_object(Bucket='dracon-lfs', Key=key)
    try: body = response['Body'].read(2097153)
    finally: response['Body'].close()
    assert len(body) == entry['bytes']
    digest = hashlib.sha256(body).hexdigest()
    assert digest == entry['sha256']
    assert body == (root / entry['sourcePath']).read_bytes()
    out = Path(tempfile.mkdtemp(prefix='ovh-independent-consumer-'))
    os.chmod(out, 0o700)
    (out / 'fetched.png').write_bytes(body)
    record = {'at':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'sdk':'boto3 '+boto3.__version__,
        'role':'consumer', 'operation':'GetObject', 'bucket':'dracon-lfs', 'key':key,
        'endpoint':profile['storage']['endpoint'], 'http':response['ResponseMetadata']['HTTPStatusCode'],
        'lastModified':response['LastModified'].isoformat(), 'etag':response.get('ETag'),
        'bytes':len(body), 'sha256':digest, 'byteIdenticalToSource':True,
        'manifestSha256':hashlib.sha256(manifest_bytes).hexdigest(), 'destination':str(out / 'fetched.png')}
    (evidence / 'independent-consumer-20261019.json').write_text(json.dumps(record,indent=2)+'\n')
    print(json.dumps(record))
except Exception:
    raise SystemExit('Independent consumer verification failed; no secrets or raw remote exceptions emitted')
finally: client.close()
