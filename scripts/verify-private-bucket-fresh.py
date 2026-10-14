#!/usr/bin/env python3
"""Archive an immutable parent revision, project only committed submodule
workspace metadata/vendors needed by the hub, install independent dependencies,
restore with the scoped consumer and run the documented hub build. No cache or
node_modules from the original checkout is used. No canonical file is modified.
"""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile


def git(repo, *args):
    return subprocess.run(['git', '-C', str(repo), *args], capture_output=True,
                          check=True, timeout=120).stdout


OMITTED_LINKS = []


def extract(data, target):
    def portable(member, destination):
        try:
            return tarfile.data_filter(member, destination)
        except (tarfile.AbsoluteLinkError, tarfile.LinkOutsideDestinationError):
            # Machine-local Nix result links are not source/dependencies and
            # must not reconnect this checkout to the original machine cache.
            OMITTED_LINKS.append(member.name)
            return None
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        archive.extractall(target, filter=portable)


def main():
    source = Path(sys.argv[1]).resolve()
    evidence = Path(sys.argv[2]).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    revision = git(source, 'rev-parse', sys.argv[3] if len(sys.argv) > 3 else 'HEAD').decode().strip()
    fresh = Path(tempfile.mkdtemp(prefix='private-bucket-fresh-'))
    extract(git(source, 'archive', revision), fresh)
    metadata = {}
    for workspace in json.loads((fresh / 'web/package.json').read_text())['workspaces']:
        if '*' in workspace or (fresh / 'web' / workspace / 'package.json').exists():
            continue
        relative = 'web/' + workspace
        tree = git(source, 'ls-tree', revision, '--', relative).decode().split()
        if len(tree) < 3 or tree[0] != '160000':
            raise RuntimeError('Missing committed workspace metadata: ' + workspace)
        commit = tree[2]
        nested = source / relative
        dest = fresh / relative
        dest.mkdir(parents=True, exist_ok=True)
        (dest / 'package.json').write_bytes(git(nested, 'show', commit + ':package.json'))
        metadata[workspace] = commit
        if git(nested, 'ls-tree', commit, '--', 'vendor'):
            extract(git(nested, 'archive', commit, '--', 'vendor'), dest)
    env = os.environ.copy()
    env['BUN_INSTALL_CACHE_DIR'] = str(fresh / '.independent-dependency-cache')
    def run(name, command, cwd, bound):
        with (evidence / (name + '.log')).open('wb') as log:
            result = subprocess.run(['timeout', str(bound), *command], cwd=cwd, env=env,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=bound + 15)
        if result.returncode:
            raise RuntimeError(name + ' failed; inspect captured log')
    # Projected submodule metadata can require a temporary lock refresh; this
    # changes only the isolated dependency environment, never canonical source.
    run('dependencies', ['bun', 'install', '--ignore-scripts'], fresh / 'web', 300)
    config = json.loads((fresh / 'bucket-assets.json').read_text())
    manifest = json.loads((fresh / config['manifest']).read_text())
    for asset in manifest['assets']:
        if asset.get('bucketed'):
            (fresh / config['assetRoot'] / asset['sourcePath']).unlink(missing_ok=True)
    run('restore', ['node', config['workflowScript'], 'restore', 'bucket-assets.json', str(fresh)], fresh, 180)
    identities = []
    for asset in manifest['assets']:
        if not asset.get('bucketed'):
            continue
        body = (fresh / config['assetRoot'] / asset['sourcePath']).read_bytes()
        sha = hashlib.sha256(body).hexdigest()
        if sha != asset['sha256'] or len(body) != asset['bytes']:
            raise RuntimeError('Restored asset mismatch')
        identities.append({'sourcePath': asset['sourcePath'], 'sha256': sha, 'bytes': len(body)})
    run('favicon-audit', ['node', 'web/games/scripts/audit-favicons.mjs'], fresh, 120)
    run('games-build', ['bun', 'run', '--filter', 'games', 'build'], fresh / 'web', 300)
    result = {'ok': True, 'parentRevision': revision, 'workspaceMetadataRevisions': metadata,
              'isolatedDirectory': str(fresh), 'originalDependencyOrAssetCacheUsed': False,
              'omittedNonportableLinks': OMITTED_LINKS,
              'restoredAssets': identities, 'buildCommand': 'bun run --filter games build', 'buildExitCode': 0}
    (evidence / 'fresh-build.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
