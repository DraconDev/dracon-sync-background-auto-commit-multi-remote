#!/usr/bin/env python3
"""Read-only initial-fleet ref receipt: BASELINE_JSON LABEL OUTPUT_JSON.

No fetch, push, index/ref/config writes, or retry changes. Each command owns a
process group and has a finite deadline. Query failures remain explicit.
"""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from urllib.parse import urlsplit


def digest(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def run(args, cwd=None, timeout=45):
    child = subprocess.Popen(args, cwd=cwd, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, start_new_session=True,
                             env={**os.environ, 'GIT_TERMINAL_PROMPT': '0'})
    try:
        stdout, stderr = child.communicate(timeout=timeout)
        return child.returncode, stdout.decode('utf8', 'replace'), stderr
    except subprocess.TimeoutExpired:
        # The leader has not been reaped; its group identity is still ours.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.communicate(timeout=10)
        return 124, '', b'command deadline exceeded'


def git(repo, *args):
    return run(['git', *args], cwd=repo)


def text(repo, *args):
    code, out, _ = git(repo, *args)
    if code:
        raise RuntimeError(f'read-only git {args[0]} exited {code}')
    return out.strip()


def safe_url(value):
    if '://' in value:
        p = urlsplit(value)
        return f'{p.scheme}://{p.hostname or ""}{p.path}'
    return value.split('@')[-1].split('?')[0].split('#')[0]


def identity():
    code, out, _ = run(['systemctl', 'show', 'dracon-sync-operator.service',
                        '-p', 'MainPID', '--value'], timeout=10)
    if code or not out.strip().isdigit() or int(out.strip()) <= 0:
        raise RuntimeError('cannot identify loaded daemon')
    pid = int(out.strip())
    return {'pid': pid, 'loaded_sha256': digest(f'/proc/{pid}/exe')}


def required_remotes(initial):
    # repos --json lists mirror targets, not the primary origin. In this
    # initial fleet origin is an operator GitHub/GitLab destination too.
    return list(dict.fromkeys(['origin', *initial['push_to_remotes']]))


def advertised_tip(code, output, branch):
    if code or not output.strip():
        return None
    fields = output.split()
    if (len(fields) != 2 or fields[1] != f'refs/heads/{branch}'
            or len(fields[0]) not in (40, 64)
            or any(c not in '0123456789abcdef' for c in fields[0])):
        raise RuntimeError('malformed advertised branch ref')
    return fields[0]


def observe(initial):
    repo = initial['repo']
    row = {'repo': repo, 'started_at': time.time(),
           'initial_head': initial['baseline_head'],
           'initial_intended_remotes': required_remotes(initial),
           'initial_excluded_remotes': initial['excluded_remotes'], 'remotes': {}}
    try:
        head = text(repo, 'rev-parse', 'HEAD')
        branch = text(repo, 'symbolic-ref', '--short', 'HEAD')
        row.update(head=head, branch=branch, head_type=text(repo, 'cat-file', '-t', head),
                   head_metadata=text(repo, 'show', '-s', '--format=%H%n%P%n%ct%n%an%n%ae', head).splitlines())
        for remote in text(repo, 'remote').splitlines():
            code, output, error = git(repo, 'ls-remote', remote, f'refs/heads/{branch}')
            tip = advertised_tip(code, output, branch)
            kind = None
            if tip:
                cat_code, cat_output, _ = git(repo, 'cat-file', '-t', tip)
                if cat_code == 0:
                    kind = cat_output.strip()
            row['remotes'][remote] = {
                'url_without_credentials': safe_url(text(repo, 'remote', 'get-url', remote)),
                'intended_at_initial_baseline': remote in required_remotes(initial),
                'query_succeeded': code == 0, 'query_exit': code,
                'query_error_sha256': hashlib.sha256(error).hexdigest() if code else None,
                'branch_ref': tip, 'object_type': kind,
                'equals_observed_head': bool(tip) and tip == head,
                'confirmed_at': time.time()}
        row['head_after_queries'] = text(repo, 'rev-parse', 'HEAD')
    except Exception as error:
        row['error'] = f'{type(error).__name__}: {error}'
    row['finished_at'] = time.time()
    return row


def main():
    baseline, label, output = sys.argv[1:]
    initial = json.loads(Path(baseline).read_text())['initial_repositories']
    if len(initial) != 10 or len({r['repo'] for r in initial}) != 10:
        raise RuntimeError('expected all ten distinct initially affected repositories')
    before = identity()
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        rows = list(pool.map(observe, initial))
    after = identity()
    payload = {'label': label, 'observed_at': time.time(),
               'observer_sha256': digest(__file__), 'daemon_before': before,
               'daemon_after': after, 'same_loaded_revision_throughout': before == after,
               'operations': 'read-only Git object and advertised-ref queries',
               'repositories': rows}
    Path(output).write_text(json.dumps(payload, indent=2) + '\n')
    print(json.dumps({'output': output, 'repositories': len(rows),
                      'same_loaded_revision_throughout': before == after,
                      'repository_errors': sum('error' in r for r in rows),
                      'failed_ref_queries': sum(not v['query_succeeded'] for r in rows for v in r['remotes'].values())}))


if __name__ == '__main__':
    main()
