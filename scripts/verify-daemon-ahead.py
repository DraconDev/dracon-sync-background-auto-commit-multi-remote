#!/usr/bin/env python3
"""Isolated clean/dirty-ahead and repeated-edit convergence regression.

No live repos, filters, credentials or network remotes. Content checks include
worktree, index, local HEAD and a bare remote. This is not a timing acceptance
substitute for verify-daemon-fairness.py.
"""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def main():
    binary = str(Path(sys.argv[1]).resolve())
    root = Path(tempfile.mkdtemp(prefix='sync-ahead-'))
    home, watch, state = (root / n for n in ('home', 'watch', 'state'))
    for path in (home, watch, state):
        path.mkdir()
    git_bin = shutil.which('git')
    identity = {key: subprocess.check_output([git_bin, 'config', '--get', key], text=True).strip()
                for key in ('user.name', 'user.email')}
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('GIT_', 'DRACON_', 'XDG_'))}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
               XDG_STATE_HOME=str(state), XDG_CACHE_HOME=str(home / '.cache'),
               GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(home / '.gitconfig'),
               GIT_TERMINAL_PROMPT='0', DRACON_SYNC_STATE_DIR=str(state),
               DRACON_SYNC_LEDGER=str(state / 'ledger.jsonl'), DRACON_SYNC_DEBUG='1')
    (home / '.gitconfig').write_text('[user]\nname = ' + json.dumps(identity['user.name'])
                                    + '\nemail = ' + json.dumps(identity['user.email']) + '\n')

    def git(*args, cwd):
        return subprocess.check_output([git_bin, *args], cwd=cwd, env=env,
                                       stderr=subprocess.STDOUT, timeout=10).decode().strip()

    repo, remote = watch / 'single', root / 'remote.git'
    repo.mkdir()
    git('init', '--bare', str(remote), cwd=root)
    git('init', '-b', 'main', cwd=repo)
    file = repo / 'seed.txt'
    file.write_text('seed\n')
    git('add', '--', 'seed.txt', cwd=repo)
    git('commit', '-m', 'fixture seed', cwd=repo)
    git('remote', 'add', 'origin', str(remote), cwd=repo)
    git('push', '-u', 'origin', 'main', cwd=repo)
    # Manual commits are fixture setup only, before the isolated daemon starts.
    file.write_text('local ahead\n')
    git('add', '--', 'seed.txt', cwd=repo)
    git('commit', '-m', 'fixture unpublished ahead commit', cwd=repo)
    if '--dirty-ahead' in sys.argv[2:]:
        file.write_text('dirty and ahead\n')
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
    env['DRACON_SYNC_POLICY'] = str(policy)
    report = {'root': str(root), 'binary': binary,
              'dirty_ahead': '--dirty-ahead' in sys.argv[2:], 'observations': []}

    def hashes():
        return {'worktree': git('hash-object', 'seed.txt', cwd=repo),
                'index': git('rev-parse', ':seed.txt', cwd=repo),
                'head': git('rev-parse', 'HEAD:seed.txt', cwd=repo),
                'remote': git('--git-dir', str(remote), 'rev-parse',
                              'main:seed.txt', cwd=root)}

    def wait_equal(phase):
        started = time.monotonic()
        while True:
            if daemon.poll() is not None:
                raise RuntimeError(f'daemon exited {daemon.returncode}')
            values = hashes()
            report['observations'].append({'phase': phase, 'unix_ms': time.time_ns() // 1000000,
                                           'elapsed': time.monotonic() - started, **values})
            if len(set(values.values())) == 1:
                return
            if time.monotonic() - started > 15:
                raise RuntimeError(f'{phase} did not converge: {values}')
            time.sleep(0.2)

    log = (root / 'daemon.log').open('w')
    daemon = subprocess.Popen([binary, '-vv', 'daemon'], env=env, stdout=log,
                              stderr=subprocess.STDOUT, start_new_session=True)
    try:
        wait_equal('initial-ahead')
        for i in (1, 2):
            file.write_text(f'edit {i}\n')
            wait_equal(f'edit-{i}')
        report['passed'] = True
    except Exception as error:
        report['passed'] = False
        report['error'] = str(error)
    finally:
        if daemon.poll() is None:
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
