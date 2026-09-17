#!/usr/bin/env python3
"""End-to-end fairness probe for the user-visible convergence failure.

Reproduces the reported symptom class with an isolated daemon, four repos and
local bare remotes: repo A is edited continuously, repo B receives one change
after startup, repo C's push is delayed by a slow wrapper (simulating a slow
remote), and repo B2 holds an uncommitted change made BEFORE the daemon
started (missed-event reconciliation). No live state, forges, credentials or
canonical repositories are touched. Warden is disabled here; this measures the
sync scheduler and its fairness, not the encryption filter.
"""
import json
import os
from pathlib import Path
import shutil
import shlex
import signal
import subprocess
import sys
import tempfile
import threading
import time

DEADLINE = 30.0


def main():
    binary = str(Path(sys.argv[1]).resolve())
    slow_filter = '--slow-filter' in sys.argv[2:]
    git_bin = shutil.which('git')
    identity = {key: subprocess.check_output([git_bin, 'config', '--get', key], text=True).strip()
                for key in ('user.name', 'user.email')}
    root = Path(tempfile.mkdtemp(prefix='sync-fairness-'))
    home, watch, state = [root / p for p in ('home', 'watch', 'state')]
    for p in (home, watch, state):
        p.mkdir()
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('GIT_', 'DRACON_', 'XDG_'))}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
               XDG_STATE_HOME=str(state), XDG_CACHE_HOME=str(home / '.cache'),
               GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(home / '.gitconfig'),
               GIT_TERMINAL_PROMPT='0', DRACON_SYNC_STATE_DIR=str(state),
               DRACON_SYNC_LEDGER=str(state / 'ledger.jsonl'))
    (home / '.gitconfig').write_text('[user]\n\tname = ' + json.dumps(identity['user.name'])
                                     + '\n\temail = ' + json.dumps(identity['user.email']) + '\n')
    events = root / 'git-events.jsonl'
    wrapper = root / 'git-probe'
    logger = root / 'git-event-probe'
    # C's push is slowed by 3s inside the wrapper: execution/transfer cost is
    # visible separately from queue wait, without touching any timeout config.
    logger.write_text('#!/usr/bin/env python3\nimport json,os,sys,time\n'
                       + f'with open({str(events)!r}, "a") as f:\n'
                       + ' f.write(json.dumps({"t":time.monotonic(),"args":sys.argv[1:],'
                       '"cwd":os.getcwd()})+"\\n")\n'
                       + 'if sys.argv[1:2] == ["push"] and "--delete" not in sys.argv and os.getcwd().endswith("/c"):\n'
                       + '    time.sleep(3)\n'
                       + f'os.execv({git_bin!r}, [{git_bin!r}]+sys.argv[1:])\n')
    logger.chmod(0o700)
    # Only instrument operations under test, not every status/config probe.
    wrapper.write_text('#!' + shutil.which('sh') + '\n'
                       + 'case "$1" in add|push|diff) exec ' + shlex.quote(str(logger)) + ' "$@";; esac\n'
                       + 'exec ' + shlex.quote(git_bin) + ' "$@"\n')
    wrapper.chmod(0o700)

    def git(*argv, cwd):
        return subprocess.check_output([git_bin, *argv], cwd=cwd, env=env,
                                       stderr=subprocess.STDOUT, timeout=10)

    names = ('a', 'b', 'b2', 'c', 'd-filter') if slow_filter else ('a', 'b', 'b2', 'c')
    repos = {}
    for name in names:
        remote = root / f'remote-{name}.git'
        repo = watch / name
        repo.mkdir()
        git('init', '--bare', str(remote), cwd=root)
        git('init', '-b', 'main', cwd=repo)
        (repo / 'seed.txt').write_text('seed\n')
        git('add', '--', 'seed.txt', cwd=repo)
        git('commit', '-m', 'seed', cwd=repo)
        git('remote', 'add', 'origin', str(remote), cwd=repo)
        git('push', '-u', 'origin', 'main', cwd=repo)
        repos[name] = repo
    if slow_filter:
        # A required clean filter with bounded execution cost. It never contacts
        # Warden or reads real secrets. Only this fixture's local config changes.
        filter_program = root / 'slow-clean.py'
        filter_program.write_text('import sys,time\ntime.sleep(4)\n'
                                  'sys.stdout.buffer.write(sys.stdin.buffer.read())\n')
        filtered_repo = repos['d-filter']
        git('config', 'filter.probe.clean',
            shlex.quote(sys.executable) + ' ' + shlex.quote(str(filter_program)), cwd=filtered_repo)
        git('config', 'filter.probe.required', 'true', cwd=filtered_repo)
        # info/attributes is fixture-local and need not be committed.
        (filtered_repo / '.git/info/attributes').write_text('seed.txt filter=probe\n')
        (filtered_repo / 'seed.txt').write_text('filtered change\n')
    # Missed-event reconciliation: repo b2 is dirty before the daemon starts.
    (repos['b2'] / 'early.txt').write_text('written before daemon start\n')

    policy = root / 'policy.toml'
    policy.write_text(f'watch_roots = [{json.dumps(str(watch))}]\n'
                      'pulse_interval_secs = 1\ninactivity_push_delay_secs = 2\n'
                      'auto_commit = true\nauto_push = true\nauto_pull = false\n'
                      'auto_bump_versions = false\nauto_harden_with_warden = false\n'
                      'auto_github_private = false\nauto_repair_concerns = false\n'
                      'auto_repair_warns = false\nauto_rewrite_large_blobs = false\n'
                      'build_artifact_cleanup = false\nstandard_files_auto = false\n'
                      'auto_tag = false\nauto_release = false\nauto_publish = false\n'
                      'auto_gc_garbage_threshold_bytes = 0\nremotes = []\n')
    env.update(DRACON_SYNC_POLICY=str(policy), DRACON_SYNC_GIT_BIN=str(wrapper),
               DRACON_SYNC_DEBUG='1')
    report = {'root': str(root), 'scope': 'fairness: continuous edits, slow push, pre-start change',
              'slow_required_filter': slow_filter}
    log = (root / 'daemon.log').open('w')
    daemon = subprocess.Popen([binary, '-vv', 'daemon'], env=env, stdout=log,
                              stderr=subprocess.STDOUT, start_new_session=True)
    stop_editing = threading.Event()
    editor = None
    try:
        start = time.monotonic()
        time.sleep(1.0)
        t_b = time.monotonic()
        (repos['b'] / 'seed.txt').write_text('changed\n')
        time.sleep(0.2)
        t_c = time.monotonic()
        (repos['c'] / 'seed.txt').write_text('changed\n')

        t_a = time.monotonic()

        def continuous_edits():
            i = 0
            while not stop_editing.is_set():
                (repos['a'] / f'note-{i}.txt').write_text(f'edit {i}\n')
                i += 1
                stop_editing.wait(0.3)

        editor = threading.Thread(target=continuous_edits)
        editor.start()

        expected = {'a': ('note-0.txt', b'edit 0\n'),
                    'b': ('seed.txt', b'changed\n'),
                    'b2': ('early.txt', b'written before daemon start\n'),
                    'c': ('seed.txt', b'changed\n')}
        if slow_filter:
            expected['d-filter'] = ('seed.txt', b'filtered change\n')
        seen, committed = {}, {}
        while time.monotonic() - start < DEADLINE:
            if daemon.poll() is not None:
                raise RuntimeError(f'daemon exited {daemon.returncode}')
            for name, (path, content) in expected.items():
                for observed, gitdir in ((committed, repos[name] / '.git'),
                                         (seen, root / f'remote-{name}.git')):
                    if name in observed:
                        continue
                    try:
                        blob = git('--git-dir', str(gitdir), 'show',
                                   f'refs/heads/main:{path}', cwd=root)
                    except subprocess.CalledProcessError:
                        continue  # The new path has not been committed yet.
                    if blob == content:
                        observed[name] = time.monotonic()
            if len(seen) == len(names):
                break
            time.sleep(0.05)
        stop_editing.set()
        editor.join(timeout=2)
        report['converged'] = len(seen) == len(names)
        report['commit_observed_seconds'] = {n: t - start for n, t in committed.items()}
        report['remote_observed_seconds'] = {n: t - start for n, t in seen.items()}
        report['a_commits'] = int(git('rev-list', '--count', 'HEAD', cwd=repos['a'])) - 1
        rows = [json.loads(line) for line in events.read_text().splitlines()]
        dispatch = {}

        def first(name, op, after=0.0):
            for row in rows:
                if row['t'] >= after and row['cwd'] == str(repos[name]) \
                        and row['args'] and row['args'][0] == op \
                        and '--delete' not in row['args']:
                    return row['t']
            return None

        for name in names:
            add = first(name, 'add')
            push = first(name, 'push')
            dispatch[name] = {'add_rel': None if add is None else add - start,
                              'push_rel': None if push is None else push - start}
        b_add = dispatch['b']['add_rel']
        c_add = dispatch['c']['add_rel']
        b_push = dispatch['b']['push_rel']
        c_push = dispatch['c']['push_rel']
        b2_add = dispatch['b2']['add_rel']
        report['dispatch'] = dispatch
        checks = {
            'b_staging_at_quiet_plus_one_pulse': b_add is not None
                and 2 <= start + b_add - t_b <= 3,
            'c_staging_at_quiet_plus_one_pulse': c_add is not None
                and 2 <= start + c_add - t_c <= 3,
            'slow_push_execution_measured_separately': c_push is not None and 'c' in seen
                and 3 <= seen['c'] - (start + c_push) <= 6,
            # HEAD polling is an upper bound on commit completion. A positive
            # overrun proves a failure; a pass needs the finer timing gate too.
            'b_push_within_one_pulse_of_observed_commit': b_push is not None and 'b' in committed
                and start + b_push - committed['b'] <= 1,
            'continuous_work_reaches_remote_within_10s': 'a' in seen and seen['a'] - t_a <= 10,
            'pre_start_change_stages_within_3s': b2_add is not None and b2_add <= 3,
        }
        if slow_filter:
            checks['slow_required_filter_eventually_converges'] = 'd-filter' in seen
        report['checks'] = checks
        report['passed'] = all(checks.values()) and report['converged']
    finally:
        stop_editing.set()
        if editor is not None:
            editor.join(timeout=2)
        os.killpg(daemon.pid, signal.SIGTERM)
        try:
            daemon.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(daemon.pid, signal.SIGKILL)
            daemon.wait(timeout=3)
        log.close()
    (root / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
