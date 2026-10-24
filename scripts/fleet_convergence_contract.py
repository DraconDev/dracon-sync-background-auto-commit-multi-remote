"""Fail-closed inventory and destination continuity for fleet evidence."""
from urllib.parse import urlsplit


def safe_url(value):
    if '://' in value:
        p = urlsplit(value)
        host = p.hostname or ''
        if ':' in host:
            host = f'[{host}]'
        if p.port is not None:
            host += f':{p.port}'
        return f'{p.scheme}://{host}{p.path}'
    return value.split('@')[-1].split('?')[0].split('#')[0]


def destination(value):
    """Compare forge destinations, not spelling/transport aliases."""
    value = safe_url(value)
    if '://' in value:
        parsed = urlsplit(value)
        host, path = parsed.hostname or '', parsed.path
        if parsed.port is not None:
            host += f':{parsed.port}'
    else:
        host, _, path = value.partition(':')
    host, path = host.lower(), path.strip('/').removesuffix('.git')
    # GitHub paths are case-insensitive; arbitrary forge paths need not be.
    if host == 'github.com':
        path = path.lower()
    return f'{host}/{path}'


def _inventory(rows, label):
    repos = [row['repo'] for row in rows]
    if not repos or any(not isinstance(repo, str) or not repo for repo in repos):
        raise ValueError(f'{label}: empty or invalid repository inventory')
    if len(repos) != len(set(repos)):
        raise ValueError(f'{label}: duplicate repository')
    return {row['repo']: row for row in rows}


def _targets(row, expected, label, has_remotes):
    targets = row['intended_destinations']
    logical = [target['logical'] for target in targets]
    if len(logical) != len(set(logical)) or set(logical) != expected:
        raise ValueError(f'{label}: missing, duplicate or extra logical destination')
    result = {}
    for target in targets:
        resolution = target['resolution']
        identity = target.get('destination')
        if resolution == 'configured-destination':
            if not isinstance(identity, str) or not identity or not target['aliases']:
                raise ValueError(f'{label}: invalid configured destination')
            if has_remotes:
                for alias in target['aliases']:
                    remote = row['remotes'].get(alias)
                    if not remote or destination(remote['url_without_credentials']) != identity:
                        raise ValueError(f'{label}: alias destination does not match declared identity')
        elif resolution != 'unresolved':
            raise ValueError(f'{label}: unknown destination resolution')
        result[target['logical']] = (resolution, identity)
    return result


def validate_fleet(initial, witnesses, before, after):
    """All three evidence sets must cover the independent initial inventory.

    Logical targets must retain the same normalized identity. Alias spellings
    may change without weakening proof of the actual destination.
    """
    baseline = _inventory(initial, 'initial')
    inventories = [
        _inventory(witnesses, 'witnesses'),
        _inventory(before, 'before'),
        _inventory(after, 'after'),
    ]
    for label, inventory in zip(('witnesses', 'before', 'after'), inventories):
        if inventory.keys() != baseline.keys():
            raise ValueError(f'{label}: repository set differs from initial inventory')
    for repo, initial_row in baseline.items():
        expected = {'origin', *initial_row['push_to_remotes']}
        targets = [_targets(inventory[repo], expected, label, index != 0)
                   for index, (label, inventory) in enumerate(zip(
                       ('witnesses', 'before', 'after'), inventories))]
        if not targets[0] == targets[1] == targets[2]:
            raise ValueError(f'{repo}: intended destination changed across evidence')


def prior_observations(row, identity):
    """Select actual successful prior refs by destination, never alias alone."""
    return [(alias, info) for alias, info in row['remotes'].items()
            if destination(info['url_without_credentials']) == identity
            and info.get('query_succeeded') and info.get('object_type') == 'commit'
            and info.get('branch_ref')]
