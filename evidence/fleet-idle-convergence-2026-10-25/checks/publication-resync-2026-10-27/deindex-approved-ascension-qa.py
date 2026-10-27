#!/usr/bin/env python3
"""One-time operator-approved exact-four maintenance; never commits or pushes."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile

REPO = Path('/home/dracon/Dev/dracon-platform/web/games/wip/doomtap')
PATHS = [f'docs/screenshots/qa/ascension-{s}.png' for s in ('after', 'armed', 'before', 'narrow')]

def git(args, env=None):
    return subprocess.check_output(['git', '-C', str(REPO), *args], env=env, timeout=30)

def durable(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'wb') as handle:
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())

def local_hash(path):
    fd = os.open(REPO / path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, 'rb') as handle:
        before = os.fstat(handle.fileno())
        assert stat.S_ISREG(before.st_mode) and before.st_size <= 104857600
        digest = hashlib.file_digest(handle, 'sha256').hexdigest()
        after = os.fstat(handle.fileno())
        assert (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)
        canonical = (REPO / path).stat(follow_symlinks=False)
        assert (canonical.st_dev, canonical.st_ino, canonical.st_size, canonical.st_mtime_ns, canonical.st_ctime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)
        return digest

index = Path(os.fsdecode(git(['rev-parse', '--path-format=absolute', '--git-path', 'index'])).strip())
lock = index.with_name(index.name + '.lock')
fd = os.open(lock, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
owned = os.fstat(fd)
try:
    head = git(['rev-parse', 'HEAD'])
    assert index.stat().st_size <= 64 * 1024 * 1024
    raw = index.read_bytes()
    state = Path.home() / '.local/state/dracon-sync/diagnostics'
    state.mkdir(parents=True, exist_ok=True, mode=0o700)
    backup = Path(tempfile.mkdtemp(prefix='doomtap-approved-four-', dir=state))
    durable(backup / 'original-index', raw)
    candidate = backup / 'candidate-index'
    durable(candidate, raw)
    env = os.environ.copy()
    env['GIT_INDEX_FILE'] = str(candidate)
    entries = git(['ls-files', '--stage', '-z'], env).split(b'\0')
    wanted = {os.fsencode(p) for p in PATHS}
    selected = [e for e in entries if e and e.split(b'\t', 1)[1] in wanted]
    for i, entry in enumerate(selected):
        metadata, _ = entry.split(b'\t', 1)
        mode, oid, stage = metadata.split()
        assert mode in (b'100644', b'100755') and stage == b'0'
        assert int(git(['cat-file', '-s', oid.decode()])) <= 104857600
        durable(backup / f'original-staged-blob-{i}', git(['cat-file', 'blob', oid.decode()]))
    for directory in (backup, state):
        directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    before = {p: local_hash(p) for p in PATHS}
    git(['update-index', '--force-remove', '--', *PATHS], env)
    remaining = git(['ls-files', '--stage', '-z'], env).split(b'\0')
    assert remaining == [e for e in entries if not e or e.split(b'\t', 1)[1] not in wanted], 'unrelated staging changed'
    assert index.read_bytes() == raw and git(['rev-parse', 'HEAD']) == head, 'foreign index/ref change'
    assert {p: local_hash(p) for p in PATHS} == before, 'producer changed a local image; no installation'
    with os.fdopen(fd, 'wb', closefd=False) as handle:
        handle.write(candidate.read_bytes())
        handle.flush()
        os.fsync(handle.fileno())
    # The ordinary Git index lock is held from the original read through this
    # final installation. Standard Git writers refuse instead of being reset.
    os.replace(lock, index)
    parent = os.open(index.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)
    after = {p: local_hash(p) for p in PATHS}
    receipt = {'scope': 'user-approved exact-four QA cache de-index; no commit or push', 'paths': PATHS,
               'headUnchanged': git(['rev-parse', 'HEAD']) == head,
               'beforeSha256': before, 'afterSha256': after, 'localBytesPreserved': before == after,
               'originalIndexSha256': hashlib.sha256(raw).hexdigest(), 'recoveryDirectory': str(backup),
               'originalStagedEntriesRetained': [os.fsdecode(e) for e in selected],
               'unrelatedStagedEntriesPreserved': True, 'publicationObserved': False}
    durable(backup / 'receipt.json', json.dumps(receipt, indent=2).encode())
    evidence = Path(__file__).with_name('doomtap-approved-four-deindex.json')
    durable(evidence, json.dumps(receipt, indent=2).encode())
    print(json.dumps(receipt, indent=2))
finally:
    os.close(fd)
    try:
        current = lock.stat(follow_symlinks=False)
        if (current.st_dev, current.st_ino) == (owned.st_dev, owned.st_ino):
            lock.unlink()
    except FileNotFoundError:
        pass
