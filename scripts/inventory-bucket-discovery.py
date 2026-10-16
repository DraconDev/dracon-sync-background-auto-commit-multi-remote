#!/usr/bin/env python3
"""Read-only fleet asset inventory. Never reads credentials or executes generators.

Input is `dracon-sync repos --json`; explicitly supply failed inspections so an
incomplete status report cannot silently narrow discovery coverage. Asset counts
are Git-visible paths, not claims of provenance, eligibility or stored bytes.
"""
import argparse
import collections
import datetime
import json
import pathlib
import subprocess

MEDIA = frozenset({'.png', '.jpg', '.jpeg', '.webp', '.avif', '.gif', '.svg',
                   '.mp3', '.wav', '.ogg', '.mp4', '.webm', '.glb', '.gltf'})
EXCLUDED = frozenset({'node_modules', 'target', 'build', 'dist', '.git',
                      '.bucket-cache', 'secrets', 'credentials', '.ssh'})


def excluded(path):
    return any(p.lower() in EXCLUDED or p.lower().startswith('.env')
               for p in pathlib.PurePosixPath(path).parts)


def config_summary(repo):
    # Mirror current root-first discovery order; do not read credential files.
    for relative in ('bucket-assets.json', '.dracon/bucket-assets.json'):
        path = repo / relative
        if any(p.is_symlink() for p in (path, *path.parents) if p != repo.parent):
            if path.exists() or path.is_symlink():
                return {'path': relative, 'error': 'symlink path refused'}
            continue
        if not path.exists():
            continue
        try:
            if not path.is_file() or path.stat().st_size > 65536:
                return {'path': relative, 'error': 'not a bounded regular config'}
            data = json.loads(path.read_text())
            return {'path': relative, 'enabled': data.get('enabled') is True,
                    'assetRoot': data.get('assetRoot'), 'manifest': data.get('manifest')}
        except (OSError, ValueError, AttributeError):
            return {'path': relative, 'error': 'config unreadable or invalid'}
    return None


def inspect(repo):
    record = {'repository': str(repo), 'configuration': config_summary(repo),
              'enrollment': 'not_qualified', 'candidateAssetRoots': [],
              'mediaPaths': 0, 'generatorNameHints': []}
    try:
        result = subprocess.run(['git', '-C', str(repo), 'ls-files', '-z',
                                 '--cached', '--others', '--exclude-standard'],
                                capture_output=True, timeout=30, check=True)
        names = sorted(set(n.decode('utf-8', errors='replace')
                           for n in result.stdout.split(b'\0') if n))
    except (OSError, subprocess.SubprocessError):
        record['reason'] = 'Git-visible file enumeration failed; explicit retry required'
        return record
    media = [n for n in names if not excluded(n)
             and pathlib.PurePosixPath(n).suffix.lower() in MEDIA]
    roots = collections.Counter('/'.join(n.split('/')[:-1][:2]) or '.'
                                for n in media)
    record['mediaPaths'] = len(media)
    record['candidateAssetRoots'] = [{'path': p, 'paths': count}
                                    for p, count in roots.most_common()]
    record['generatorNameHints'] = [n for n in names if not excluded(n)
                                    and 'generat' in n.lower()
                                    and pathlib.PurePosixPath(n).suffix
                                    in {'.py', '.mjs', '.js', '.ts', '.sh'}][:20]
    if not media:
        record['reason'] = 'No Git-visible supported media candidates'
    elif record['configuration'] and record['configuration'].get('enabled'):
        record['reason'] = ('Explicit workflow exists; existing registrations do not '
                            'prove discovery eligibility for additional files')
    else:
        record['reason'] = ('Media candidates found; no verified generator and consumer '
                            'capability established by this read-only inventory')
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('status_json', type=pathlib.Path)
    parser.add_argument('--failed-repo', action='append', default=[])
    parser.add_argument('--output', type=pathlib.Path, required=True)
    args = parser.parse_args()
    data = json.loads(args.status_json.read_text())
    repos = {r['repo'] for r in data['rows']} | set(args.failed_repo)
    result = {'schemaVersion': 1,
              'observedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'scope': 'Git-visible tracked and nonignored untracked paths; nested repos separate',
              'statusRows': len(data['rows']), 'statusFailures': data.get('failures', 0),
              'explicitFailedRepositories': sorted(args.failed_repo),
              'repositoryCount': len(repos),
              'warning': 'Filename hints and extensions never establish enrollment eligibility',
              'repositories': [inspect(pathlib.Path(p)) for p in sorted(repos)]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'repositories': len(repos), 'output': str(args.output)}))


if __name__ == '__main__':
    main()
