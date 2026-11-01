#!/usr/bin/env python3
"""Read-only inventory over an explicit repo manifest; NEVER deletion authority.

Resolve every checkout with Git (including linked worktrees), then include nested
module/worktree gitdirs. Do not traverse object databases during discovery.
Filesystem failures and concurrent changes are explicit, not zero-byte readings.
Recovery allocation counts data/directory blocks once per inode, not net savings.
"""
import argparse
import collections
import datetime
import fcntl
import json
import os
from pathlib import Path
import stat
import subprocess
import time

PREFIXES = ('dracon-staging-', 'dracon-epoch-message-')


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def failure(errors, path, error):
    errors.append({'path': str(path), 'error': str(error)})


def signature(s):
    return (s.st_dev, s.st_ino, s.st_mode, s.st_size, s.st_mtime_ns,
            s.st_ctime_ns, s.st_nlink)


def measure(root):
    """No symlink following; verify every sampled stat again after enumeration."""
    errors, observed, inodes = [], {}, {}
    logical = files = packs = 0
    todo = [Path(root)]
    while todo:
        p = todo.pop()
        try:
            s = p.lstat()
            observed[p] = signature(s)
            inodes[(s.st_dev, s.st_ino)] = s.st_blocks * 512
            if stat.S_ISDIR(s.st_mode):
                with os.scandir(p) as entries:
                    todo.extend(Path(e.path) for e in entries)
            elif stat.S_ISREG(s.st_mode):
                logical += s.st_size
                files += 1
                packs += p.name.endswith('.pack')
            elif stat.S_ISLNK(s.st_mode):
                failure(errors, p, 'symlink not followed; unsupported recovery input')
            else:
                failure(errors, p, 'non-regular filesystem input')
        except OSError as e:
            failure(errors, p, e)
    for p, before in observed.items():
        try:
            if signature(p.lstat()) != before:
                failure(errors, p, 'changed during observation')
        except OSError as e:
            failure(errors, p, e)
    return {'logical_bytes_observed': logical, 'logical_bytes': logical if not errors else None,
            'regular_files': files, 'pack_files': packs, 'errors': errors}, inodes


def gitdirs(repos, errors):
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null',
               GIT_TERMINAL_PROMPT='0', GIT_NO_LAZY_FETCH='1')
    pending = []
    for repo in repos:
        try:
            result = subprocess.run(['git', '-C', repo, 'rev-parse', '--absolute-git-dir'],
                                    env=env, capture_output=True, timeout=15, check=True)
            pending.append(Path(os.fsdecode(result.stdout.rstrip(b'\n'))))
        except (OSError, subprocess.SubprocessError) as e:
            failure(errors, repo, e)
    found = {}
    while pending:
        p = pending.pop()
        try:
            p = p.resolve(strict=True)
            s = p.stat()
            if not stat.S_ISDIR(s.st_mode):
                raise ValueError('Git metadata path is not a directory')
            key = (s.st_dev, s.st_ino)
            if key in found:
                continue
            found[key] = p
            # A linked worktree's common directory may be outside its checkout.
            common = p / 'commondir'
            if os.path.lexists(common):
                target = os.fsdecode(common.read_bytes().rstrip(b'\n'))
                pending.append(p / target)
            for container in ('modules', 'worktrees'):
                base = p / container
                if not os.path.lexists(base):
                    continue
                if not base.is_dir() or base.is_symlink():
                    raise ValueError(f'unsupported metadata container: {base}')
                for current, dirs, names in os.walk(
                        base, followlinks=False,
                        onerror=lambda e: failure(errors, e.filename, e)):
                    q = Path(current)
                    if 'HEAD' in names and ('objects' in dirs or 'commondir' in names):
                        pending.append(q)
                        dirs[:] = []
                    else:
                        for name in dirs:
                            if (q / name).is_symlink():
                                failure(errors, q / name, 'metadata directory symlink not followed')
                        dirs[:] = [n for n in dirs if n not in ('objects', 'logs', 'refs', 'hooks', 'info')
                                   and not (q / n).is_symlink()]
        except (OSError, ValueError) as e:
            failure(errors, p, e)
    return sorted(found.values())


def optional_stat(path):
    try:
        return path.lstat()
    except FileNotFoundError:
        return None


def protocol_file(s):
    return (stat.S_ISREG(s.st_mode) and s.st_uid == os.geteuid()
            and s.st_nlink == 1 and not s.st_mode & 0o022)


def ownership(path, age_floor):
    """Momentary diagnostic only. Release any sampled lock before returning."""
    directory = path.lstat()
    if not stat.S_ISDIR(directory.st_mode):
        return 'unreadable_failed_proof', 'source is not a real directory'
    if any(optional_stat(path / n) is not None for n in ('preserve-rejected', 'original-index')):
        return 'preserve_marked', 'preservation marker present'
    if time.time() - directory.st_mtime < age_floor:
        return 'active_fresh', 'below scheduling floor; age is not inactivity proof'
    lock, marker = path / 'reclaim-owner.lock', path / 'reclaim-settled.json'
    ls, ms = optional_stat(lock), optional_stat(marker)
    if ls is None:
        return 'ownership_uncertain_legacy', 'no producer-compatible lock'
    if ms is None:
        return 'ownership_uncertain_protocol', 'no settlement proof; live owner not sampled'
    if not protocol_file(ls) or not protocol_file(ms):
        return 'unreadable_failed_proof', 'invalid protocol-file type, owner, sharing or mode'
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    with os.fdopen(os.open(lock, flags), 'rb') as held:
        if signature(os.fstat(held.fileno())) != signature(ls):
            return 'unreadable_failed_proof', 'lock changed during open'
        try:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return 'active_owner', 'compatible fence busy (producer or reclaimer)'
        with os.fdopen(os.open(marker, flags), 'rb') as source:
            if signature(os.fstat(source.fileno())) != signature(ms):
                return 'unreadable_failed_proof', 'settlement marker changed during open'
            identity = json.loads(source.read(4097))
        expected = dict(version=1, directory_device=directory.st_dev,
                        directory_inode=directory.st_ino, lock_device=ls.st_dev, lock_inode=ls.st_ino)
        if identity != expected or signature(path.lstat()) != signature(directory):
            return 'ownership_uncertain_protocol', 'settlement identity mismatch or source changed'
        if signature(lock.lstat()) != signature(ls) or signature(marker.lstat()) != signature(ms):
            return 'unreadable_failed_proof', 'protocol paths changed during observation'
        return 'ownership_eligible_candidate', 'momentary fence/marker check only; full lossless proof still required'


def inventory(manifest, age_floor):
    started = utc()
    errors, rows, stores, allocated = [], [], [], {}
    repos = manifest['repos']
    if len(set(repos)) != len(repos) or not all(Path(p).is_absolute() for p in repos):
        raise ValueError('manifest requires unique absolute checkout paths')
    directories = gitdirs(repos, errors)
    for directory in directories:
        try:
            entries = sorted(directory.iterdir())
        except OSError as e:
            failure(errors, directory, e)
            continue
        for path in entries:
            if path.name.startswith(PREFIXES):
                footprint, _ = measure(path)
                try:
                    category, reason = ownership(path, age_floor)
                except (OSError, ValueError) as e:
                    category, reason = 'unreadable_failed_proof', str(e)
                    failure(footprint['errors'], path, e)
                if footprint['errors']:
                    category, reason = 'unreadable_failed_proof', 'incomplete or changing observation'
                rows.append(dict(path=str(path), gitdir=str(directory), category=category,
                                 reason=reason, **footprint))
            elif path.name == 'dracon-rescue':
                footprint, blocks = measure(path)
                allocated.update(blocks)
                stores.append(dict(path=str(path), gitdir=str(directory), **footprint))
    categories = dict(collections.Counter(row['category'] for row in rows))
    complete = not errors and not any(r['errors'] for r in rows + stores)
    return dict(schema_version=1, started_utc=started, ended_utc=utc(), manifest=manifest,
                age_floor_secs=age_floor, deletion_authority=False, complete=complete,
                discovery_errors=errors, gitdirs=[str(p) for p in directories],
                spool_count=len(rows), categories=categories,
                spool_logical_bytes=sum(r['logical_bytes_observed'] for r in rows) if complete else None,
                recovery_logical_bytes=sum(r['logical_bytes_observed'] for r in stores) if complete else None,
                recovery_allocated_bytes_inode_dedup=sum(allocated.values()) if complete else None,
                accounting_note='allocated data/directory blocks; inode-table overhead and reflink sharing not measured; NOT net reclamation',
                spools=rows, recovery_stores=stores)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--age-floor', type=int, default=3600)
    args = parser.parse_args()
    if args.age_floor < 0:
        parser.error('negative scheduling floor')
    report = inventory(json.loads(args.manifest.read_text()), args.age_floor)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ('complete', 'spool_count', 'spool_logical_bytes', 'categories')}))
    raise SystemExit(0 if report['complete'] else 2)


if __name__ == '__main__':
    main()
