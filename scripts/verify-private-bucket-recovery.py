#!/usr/bin/env python3
"""Recover scoped keys from committed encrypted configuration, then prove S3
access with isolated clients. Temporary recovered files are 0600 in a 0700 dir.
Only nonsecret evidence is emitted; no private identity or key bytes are logged.
"""
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path.home() / '.dracon'
QUALIFIER = Path('/home/dracon/Dev/dracon-platform/web/scripts/bucket-private-qualify.mjs')


def run(command, data=None):
    result = subprocess.run(command, input=data, cwd=ROOT, capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError('Recovery command refused')
    return result.stdout


def verify():
    evidence = {}
    with tempfile.TemporaryDirectory(prefix='dracon-scoped-recovery-') as temporary:
        directory = Path(temporary)
        os.chmod(directory, 0o700)
        for role in ['uploader', 'consumer']:
            name = f'secrets/ovh/dracon-lfs-{role}.env'
            encrypted = run(['git', 'show', 'HEAD:' + name])
            recovered = run(['dracon-warden', 'filter-smudge', name], encrypted)
            if b'[DRACON_SECRET:' not in encrypted or b'[DRACON_SECRET:' in recovered:
                raise RuntimeError('Encrypted recovery proof refused')
            values = [line.split(b'=', 1)[1] for line in recovered.splitlines() if b'=' in line]
            if len(values) != 2 or any(value in encrypted for value in values):
                raise RuntimeError('Tracked plaintext refusal')
            def master(match):
                return run(['age', '-d', '-i', str(ROOT / 'keys/master.age')], base64.b64decode(match.group(1)))
            master_recovered = re.sub(rb'\[DRACON_SECRET:([^\]]+)\]', master, encrypted)
            if recovered != master_recovered:
                raise RuntimeError('Master recipient recovery refused')
            target = directory / f'dracon-lfs-{role}.env'
            descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, 'wb') as file:
                file.write(recovered)
            evidence[role] = {'headEncrypted': True, 'localRecovery': True,
                              'masterRecovery': True, 'trackedPlaintext': False,
                              'blob': run(['git', 'rev-parse', 'HEAD:' + name]).decode().strip()}
        test = subprocess.run(['node', str(QUALIFIER), str(directory)],
                              capture_output=True, timeout=240)
        cloud = json.loads(test.stdout)
        if test.returncode or cloud.get('ok') is not True:
            raise RuntimeError('Recovered-key cloud qualification refused')
        return {'ok': True, 'configurationCommit': run(['git', 'rev-parse', 'HEAD']).decode().strip(),
                'recovery': evidence, 'recoveredKeyCloudVerification': cloud,
                'offBoxCustodyVerified': False}


if __name__ == '__main__':
    try:
        print(json.dumps(verify(), indent=2))
    except Exception as error:
        print(json.dumps({'ok': False, 'code': 'ENCRYPTED_RECOVERY_REFUSED', 'errorType': type(error).__name__}))
        raise SystemExit(1)
