#!/usr/bin/env python3
"""Emit reviewed OVH S3 user policies, not generic AWS IAM provisioning.
Source: OVH S3 IAM guide, corrected against live policy-validator schema
2026-10-14: GetObjectVersionAttributes is documented but rejected;
DeleteIntelligentTieringConfiguration and RestoreObject are accepted.
Explicit deny is necessary because OVH may fall back to owner/ACL grants.
These users MUST NOT own buckets or inherit broad ACL grants.
"""
import json
import sys

SUPPORTED = '''AbortMultipartUpload BypassGovernanceRetention CreateBucket
DeleteBucket DeleteObject DeleteBucketTagging DeleteBucketWebsite DeleteObjectTagging
DeleteIntelligentTieringConfiguration RestoreObject
GetBucketAcl GetBucketCORS GetBucketLocation GetBucketLogging
GetBucketObjectLockConfiguration GetBucketTagging GetBucketVersioning GetBucketWebsite
GetEncryptionConfiguration GetIntelligentTieringConfiguration GetLifecycleConfiguration
GetObject GetObjectAcl GetObjectAttributes GetObjectLegalHold
GetObjectRetention GetObjectTagging GetReplicationConfiguration ListAllMyBuckets
ListBucket ListBucketMultipartUploads ListMultipartUploadParts ListBucketVersions
PutBucketAcl PutBucketCORS PutBucketLogging PutBucketObjectLockConfiguration
PutBucketTagging PutBucketVersioning PutBucketWebsite PutEncryptionConfiguration
PutIntelligentTieringConfiguration PutLifecycleConfiguration PutObject PutObjectAcl
PutObjectLegalHold PutObjectRetention PutObjectTagging PutReplicationConfiguration'''.split()


def policy(role, bucket='dracon-lfs', prefix='games/'):
    if role not in ('uploader', 'consumer') or bucket != 'dracon-lfs' or prefix != 'games/':
        raise ValueError('Only the reviewed private games pilot is supported')
    bucket_actions = ['ListBucket', 'GetBucketLocation']
    object_actions = ['GetObject'] + (['PutObject'] if role == 'uploader' else [])
    allowed = set(bucket_actions + object_actions)
    return {'Statement': [
        {'Effect': 'Allow', 'Action': ['s3:' + a for a in bucket_actions],
         'Resource': ['arn:aws:s3:::' + bucket]},
        {'Effect': 'Allow', 'Action': ['s3:' + a for a in object_actions],
         'Resource': ['arn:aws:s3:::' + bucket + '/' + prefix + '*']},
        {'Effect': 'Deny', 'Action': ['s3:' + a for a in sorted(set(SUPPORTED) - allowed)],
         'Resource': ['*']},
    ]}


if __name__ == '__main__':
    print(json.dumps(policy(sys.argv[1]), indent=2))
