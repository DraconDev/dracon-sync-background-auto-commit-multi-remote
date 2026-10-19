#!/usr/bin/env python3
"""Read-only consumer HeadObject probe for the retry-qualification fixture.
Prints only status/timing; no secrets, no writes. Exit 0 iff object exists."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import boto3
from botocore.config import Config

here = Path(__file__).resolve().parents[1]
evidence = here / 'evidence/bucket-live-qual-2026-10-18'
input_record = json.loads((evidence / 'retry-input-20261019.json').read_text())
profile = json.loads((Path.home() / '.dracon/state/bucket-runner/profile.json').read_text())
key = ('dracon-platform/f5829561-d50d-4644-8e6e-ad7e293fa1db/'
       + input_record['sha256'] + '.png')
creds_path = Path(profile['storage']['consumerCredentialFile'])
assert not creds_path.is_symlink() and creds_path.stat().st_uid == os.getuid()
assert creds_path.stat().st_mode & 0o077 == 0
fields = dict(line.split('=', 1) for line in creds_path.read_text().strip().splitlines())
# Probe the REAL bucket regardless of the locally injected profile endpoint.
client = boto3.client('s3', endpoint_url='https://s3.uk.io.cloud.ovh.net', region_name='uk',
    aws_access_key_id=fields['OVH_ACCESS_KEY_ID'], aws_secret_access_key=fields['OVH_SECRET_ACCESS_KEY'],
    config=Config(s3={'addressing_style': 'path'}, retries={'max_attempts': 0},
                  connect_timeout=10, read_timeout=20))
try:
    try:
        r = client.head_object(Bucket='dracon-lfs', Key=key)
        record = {'at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  'operation': 'HeadObject', 'bucket': 'dracon-lfs', 'key': key,
                  'exists': True, 'http': r['ResponseMetadata']['HTTPStatusCode'],
                  'contentLength': r.get('ContentLength'),
                  'lastModified': r['LastModified'].isoformat()}
    except client.exceptions.ClientError as e:
        record = {'at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  'operation': 'HeadObject', 'bucket': 'dracon-lfs', 'key': key,
                  'exists': False, 'errorCode': e.response.get('Error', {}).get('Code'),
                  'http': e.response.get('ResponseMetadata', {}).get('HTTPStatusCode')}
    print(json.dumps(record))
    raise SystemExit(0 if record['exists'] else 3)
finally:
    client.close()
