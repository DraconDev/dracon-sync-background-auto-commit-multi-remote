#!/usr/bin/env python3
"""Read-only epoch-spool inventory with a fixed, stated scope.

Scope: the daemon's configured watch roots, plus every nested git directory
under them. Spool directories are `<gitdir>/dracon-staging-*` and
`<gitdir>/dracon-epoch-message-*`. Nothing is modified.
"""
import json
import os
import pathlib
import sys
import time

SKIP = {'node_modules', 'target', '.cargo', '.rustup', 'vendor', '.cache', '.local',
        'dist', 'build', 'archives', 'Trash', '.npm', '.venv', 'venv', '__pycache__',
        '.gradle', '.m2', '.nix-profile', 'objects', 'logs', 'refs', 'hooks', 'info',
        'worktrees', 'modules'}
ROOTS = ['/home/dracon/.dracon', '/home/dracon/Dev', '/home/dracon/dracon/bridge-fixture']


def is_gitdir(path):
    return (path / 'HEAD').is_file() and (path / 'objects').is_dir()


def spools(root):
    found = []
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        current = pathlib.Path(dirpath)
        inside_gitdir = is_gitdir(current)
        names = [n for n in dirnames if n.startswith('dracon-staging-') or n.startswith('dracon-epoch-message-')]
        for name in names:
            path = current / name
            total = 0
            files = 0
            packs = 0
            try:
                for sub, _dirs, entries in os.walk(path):
                    for entry in entries:
                        try:
                            stat = (pathlib.Path(sub) / entry).lstat()
                            total += stat.st_size
                            files += 1
                            if entry.endswith('.pack'):
                                packs += 1
                        except OSError:
                            pass
                modified = path.lstat().st_mtime
                found.append({'gitdir': str(current), 'spool': name, 'path': str(path),
                              'bytes': total, 'files': files, 'packs': packs, 'mtime': modified,
                              'owner_lock': (path / 'reclaim-owner.lock').exists(),
                              'settled': (path / 'reclaim-settled.json').exists(),
                              'preserve_rejected': (path / 'preserve-rejected').exists(),
                              'original_index': (path / 'original-index').exists()})
            except OSError as error:
                found.append({'gitdir': str(current), 'spool': name, 'path': str(path), 'error': str(error)})
        keep = []
        for name in dirnames:
            if name.startswith('dracon-staging-') or name.startswith('dracon-epoch-message-'):
                continue
            if inside_gitdir:
                # Inside a git directory only nested git directories matter.
                if is_gitdir(current / name):
                    keep.append(name)
                continue
            if name in SKIP:
                continue
            keep.append(name)
        dirnames[:] = keep
    return found


def main():
    rows = []
    for root in ROOTS:
        if pathlib.Path(root).exists():
            rows.extend(spools(root))
    by_gitdir = {}
    for row in rows:
        entry = by_gitdir.setdefault(row['gitdir'], {'count': 0, 'bytes': 0, 'marked': 0,
                                                     'owner_lock': 0, 'settled': 0})
        entry['count'] += 1
        entry['bytes'] += row.get('bytes', 0)
        entry['marked'] += 1 if (row.get('preserve_rejected') or row.get('original_index')) else 0
        entry['owner_lock'] += 1 if row.get('owner_lock') else 0
        entry['settled'] += 1 if row.get('settled') else 0
    total = sum(row.get('bytes', 0) for row in rows)
    report = {'taken_unix': int(time.time()), 'taken_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
              'scope': {'watch_roots': ROOTS, 'rule': 'gitdir children named dracon-staging-*/dracon-epoch-message-*'},
              'spool_count': len(rows), 'total_bytes': total,
              'total_gib': round(total / 2**30, 4), 'by_gitdir': by_gitdir, 'spools': rows}
    output = pathlib.Path(sys.argv[1])
    output.write_text(json.dumps(report, indent=1) + '\n')
    print(f"spools={len(rows)} bytes={total} gib={round(total / 2**30, 4)} -> {output}")


if __name__ == '__main__':
    main()
