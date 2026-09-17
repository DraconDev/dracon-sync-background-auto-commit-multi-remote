#!/usr/bin/env python3
"""Isolated daemon dispatch probe: no live state, forges, credentials or repos.

Uses a local bare remote and the caller's existing Git identity. This initial
plain-file probe deliberately excludes Warden; it measures the sync scheduler.
Artifacts persist in a printed temporary directory for diagnosis.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    parser.add_argument('--deadline', type=float, default=20)
    parser.add_argument('--files', type=int, default=1)
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    git_bin = shutil.which('git')
    identity = {key: subprocess.check_output([git_bin, 'config', '--get', key], text=True).strip()
                for key in ('user.name', 'user.email')}
    root = Path(tempfile.mkdtemp(prefix='sync-cadence-'))
    home, watch, remote, state = [root / p for p in ('home', 'watch', 'remote.git', 'state')]
    for p in (home, watch, state):
        p.mkdir()
    repo = watch / 'sample'
    repo.mkdir()
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('GIT_', 'DRACON_', 'XDG_'))}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
               XDG_STATE_HOME=str(state), XDG_CACHE_HOME=str(home / '.cache'),
               GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(home / '.gitconfig'),
               GIT_TERMINAL_PROMPT='0', DRACON_SYNC_STATE_DIR=str(state),
               DRACON_SYNC_LEDGER=str(state / 'ledger.jsonl'))
    # Copy identity values, never alter the caller's configured identity.
    (home / '.gitconfig').write_text('[user]\n\tname = ' + json.dumps(identity['user.name'])
                                   + '\n\temail = ' + json.dumps(identity['user.email']) + '\n')

    def git(*argv, cwd=repo):
        return subprocess.check_output([git_bin, *argv], cwd=cwd, env=env,
                                       stderr=subprocess.STDOUT, timeout=10)

    git('init', '--bare', str(remote), cwd=root)
    git('init', '-b', 'main')
    (repo / 'sample.txt').write_text('initial\n')
    git('add', '--', 'sample.txt')
    git('commit', '-m', 'isolated fixture')
    git('remote', 'add', 'origin', str(remote))
    git('push', '-u', 'origin', 'main')
    events = root / 'git-events.jsonl'
    wrapper = root / 'git-probe'
    wrapper.write_text('#!/usr/bin/env python3\nimport json,os,sys,time\n'
                       + f'with open({str(events)!r}, "a") as f:\n'
                       + ' f.write(json.dumps({"t":time.monotonic(),"args":sys.argv[1:]})+"\\n")\n'
                       + f'os.execv({git_bin!r}, [{git_bin!r}]+sys.argv[1:])\n')
    wrapper.chmod(0o700)
    policy = root / 'policy.toml'
    policy.write_text(f'watch_roots = [{json.dumps(str(watch))}]\n'
                      'pulse_interval_secs = 1\ninactivity_push_delay_secs = 2\n'
                      'auto_commit = true\nauto_push = true\nauto_pull = false\n'
                      'auto_bump_versions = false\nauto_harden_with_warden = false\n'
                      'auto_github_private = false\nauto_repair_concerns = false\n'
                      'auto_repair_warns = false\nauto_rewrite_large_blobs = false\n'
                      'build_artifact_cleanup = false\nstandard_files_auto = false\n'
                      'auto_tag = false\nauto_release = false\nauto_publish = false\n'
                      'auto_gc_garbage_threshold_bytes = 0\n'
                      'remotes = []\n')
    env.update(DRACON_SYNC_POLICY=str(policy), DRACON_SYNC_GIT_BIN=str(wrapper),
               DRACON_SYNC_DEBUG='1')
    report = {'root': str(root), 'binary': binary, 'scope': 'plain-file scheduler, local bare remote'}
    with (root / 'daemon.log').open('w') as log:
        daemon = subprocess.Popen([binary, '-vv', 'daemon'], env=env, stdout=log,
                                  stderr=subprocess.STDOUT, start_new_session=True)
        try:
            time.sleep(2)
            changed = time.monotonic()
            for index in range(max(0, args.files - 1)):
                (repo / f'fixture-{index}.txt').write_text(f'synthetic file {index}\n')
            (repo / 'sample.txt').write_text('changed\n')
            report['file_count'] = args.files
            report['write_finished_monotonic'] = time.monotonic()
            report['change_monotonic'] = changed
            commit_at = remote_at = None
            while time.monotonic() - changed < args.deadline:
                if daemon.poll() is not None:
                    raise RuntimeError(f'daemon exited {daemon.returncode}')
                if commit_at is None and git('show', 'HEAD:sample.txt') == b'changed\n':
                    commit_at = time.monotonic()
                if git('--git-dir', str(remote), 'show', 'refs/heads/main:sample.txt') == b'changed\n':
                    remote_at = time.monotonic()
                    break
                time.sleep(0.05)
            rows = [json.loads(line) for line in events.read_text().splitlines()] if events.exists() else []
            dispatch = {}
            for row in rows:
                if row['t'] >= changed:
                    for op in ('add', 'commit', 'push'):
                        if op in row['args'] and not (op == 'push' and '--delete' in row['args']):
                            dispatch.setdefault(op, row['t'] - changed)
            report.update(dispatch_seconds=dispatch,
                          commit_seconds=None if commit_at is None else commit_at - changed,
                          remote_seconds=None if remote_at is None else remote_at - changed)
            report['passed'] = (remote_at is not None and 2 <= dispatch.get('add', 999) <= 3
                                and commit_at is not None
                                and 0 <= changed + dispatch.get('push', 999) - commit_at <= 1)
            report['commit_timestamp_note'] = 'polled HEAD observation; up to polling interval late'
        finally:
            os.killpg(daemon.pid, signal.SIGTERM)
            try:
                daemon.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(daemon.pid, signal.SIGKILL)
                daemon.wait(timeout=3)
    (root / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
