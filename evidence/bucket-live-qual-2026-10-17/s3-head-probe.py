#!/usr/bin/env python3
"""Read-only S3 permission probe. Loads scoped credential files, performs
only HEAD/GET (never PUT/DELETE), prints only HTTP statuses and digests.
Secrets are never printed, logged, or written."""
import hashlib
import sys
import urllib.request
import xml.etree.ElementTree as ET
from datetime import datetime, timezone

ENDPOINT = 'https://s3.sbg.perf.cloud.ovh.net'
BUCKET = 'dracon-lfs'
NS = 'dracon-platform/f5829561-d50d-4644-8e6e-ad7e293fa1db'
ICON_KEY = NS + '/a2efd455ecd8d42f4eb1a45ffeb858b07cf54e5ff3552f739878c49723229de1.png'
QUAL_KEY = NS + '/1e45ffa1583d7b12c7c99ce43110905cd80fb8fb0479cd66172f3cd6fd836f4c.png'


def load_creds(path):
    creds = {}
    with open(path, 'r', encoding='utf-8') as f:
        for line in f.read().strip().split('\n'):
            k, _, v = line.partition('=')
            if k in ('OVH_ACCESS_KEY_ID', 'OVH_SECRET_ACCESS_KEY'):
                if k in creds:
                    raise ValueError('duplicate field')
                creds[k] = v
    if set(creds) != {'OVH_ACCESS_KEY_ID', 'OVH_SECRET_ACCESS_KEY'}:
        raise ValueError('incomplete credentials')
    return creds


def sig_headers(creds, method, key):
    import hmac
    now = datetime.now(timezone.utc)
    amz = now.strftime('%Y%m%dT%H%M%SZ')
    day = now.strftime('%Y%m%d')
    url = f'{ENDPOINT}/{BUCKET}/{key}'
    req = urllib.request.Request(url, method=method)
    req.add_header('x-amz-date', amz)
    req.add_header('x-amz-content-sha256', hashlib.sha256(b'').hexdigest())
    ch = [method, '/', f'{BUCKET}/{key}', '', f'host:s3.sbg.perf.cloud.ovh.net\nx-amz-content-sha256:{hashlib.sha256(b"").hexdigest()}\nx-amz-date:{amz}\n',
          'host;x-amz-content-sha256;x-amz-date', hashlib.sha256(b'').hexdigest()]
    # Simplified SigV4 omitted: use unsigned probe (endpoint returns 403/400
    # shape either way); real auth test uses boto3 below if available.
    return req


def probe(name, cred_path, method, key):
    creds = load_creds(cred_path)
    try:
        import boto3
        import botocore
        client = boto3.client('s3', endpoint_url=ENDPOINT, region_name='sbg',
                              aws_access_key_id=creds['OVH_ACCESS_KEY_ID'],
                              aws_secret_access_key=creds['OVH_SECRET_ACCESS_KEY'])
        if method == 'HEAD':
            try:
                client.head_object(Bucket=BUCKET, Key=key)
                print(f'{name}: HEAD -> 200')
            except botocore.exceptions.ClientError as e:
                print(f'{name}: HEAD -> {e.response["ResponseMetadata"]["HTTPStatusCode"]} ({e.response["Error"]["Code"]})')
        else:
            try:
                r = client.get_object(Bucket=BUCKET, Key=key)
                body = r['Body'].read()
                print(f'{name}: GET -> 200 bytes={len(body)} sha256={hashlib.sha256(body).hexdigest()[:16]}')
            except botocore.exceptions.ClientError as e:
                print(f'{name}: GET -> {e.response["ResponseMetadata"]["HTTPStatusCode"]} ({e.response["Error"]["Code"]})')
    except ImportError:
        req = sig_headers(creds, method, key)
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                print(f'{name}: {method} -> {r.status} (unsigned probe)')
        except urllib.error.HTTPError as e:
            print(f'{name}: {method} -> {e.code} (unsigned probe)')


UP = '/home/dracon/.dracon/secrets/ovh/dracon-lfs-uploader.env'
CO = '/home/dracon/.dracon/secrets/ovh/dracon-lfs-consumer.env'
probe('uploader HEAD qual-absent', UP, 'HEAD', QUAL_KEY)
probe('uploader HEAD icon-present', UP, 'HEAD', ICON_KEY)
probe('uploader GET icon-present', UP, 'GET', ICON_KEY)
probe('consumer HEAD icon-present', CO, 'HEAD', ICON_KEY)
probe('consumer GET icon-present', CO, 'GET', ICON_KEY)
print('probe complete; no writes performed')
