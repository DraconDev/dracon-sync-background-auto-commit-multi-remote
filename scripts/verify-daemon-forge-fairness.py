#!/usr/bin/env python3
"""Restart-latency probe: forge-existence probing on the first cycle.

Phase A seeds a dirty repo and requires DAEMON convergence (remote tip moves
past the seed commit) so the forge-existence cache is genuinely warmed, then
stops the daemon. Phase B makes a second edit, restarts the daemon, and
measures how long the dirty repo waits for its first dispatch. With an
in-memory-only existence cache the restarted daemon re-runs serial
`ls-remote` forge probes (simulated with a 1.5s wrapper sleep per call)
before inspecting later repos; a durable cache skips them.

Usage: restart_latency.py <binary> <out.json>
Pass: r3's first dispatch within 3.0s of the daemon's first pulse (quiet 2s
+ one 1s pulse). Fail-before expectation: >= 4.5s (3 x 1.5s serial probes).
"""
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

LS_REMOTE_SLEEP = 1.5
DISPATCH_BUDGET_S = 3.0
PHASE_A_TIMEOUT = 90.0
PHASE_B_TIMEOUT = 45.0


def main():
    binary = str(Path(sys.argv[1]).resolve())
    out_path = Path(sys.argv[2]).resolve()
    git_bin = shutil.which('git')
    identity = {k: subprocess.check_output([git_bin, 'config', '--get', k], text=True).strip()
                for k in ('user.name', 'user.email')}
    root = Path(tempfile.mkdtemp(prefix='sync-restart-'))
    home, watch, state = [root / p for p in ('home', 'watch', 'state')]
    for p in (home, watch, state):
        p.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith(('GIT_', 'DRACON_', 'XDG_'))}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
               XDG_STATE_HOME=str(state), XDG_CACHE_HOME=str(home / '.cache'),
               GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(home / '.gitconfig'),
               GIT_TERMINAL_PROMPT='0', DRACON_SYNC_STATE_DIR=str(state),
               DRACON_SYNC_LEDGER=str(state / 'ledger.jsonl'))
    (home / '.gitconfig').write_text('[user]\n\tname = ' + json.dumps(identity['user.name'])
                                     + '\n\temail = ' + json.dumps(identity['user.email']) + '\n')
    events = root / 'git-events.jsonl'
    logger = root / 'git-event-probe'
    logger.write_text('#!/usr/bin/env python3\nimport json,os,subprocess,sys,time\n'
                      'def event(phase, **extra):\n'
                      f' with open({str(events)!r}, "a") as f:\n'
                      '  f.write(json.dumps({"t":time.monotonic(),"args":sys.argv[1:],'
                      '"phase":phase,**extra})+"\\n")\n'
                      'event("start")\n'
                      f'rc=subprocess.call([{git_bin!r}]+sys.argv[1:])\n'
                      'event("end", returncode=rc)\nsys.exit(rc)\n')
    logger.chmod(0o700)
    wrapper = root / 'git-probe'
    wrapper.write_text('#!' + shutil.which('sh') + '\n'
                       'case "$1" in\n'
                       '  ls-remote) ' + str(logger) + ' "$@"; sleep ' + str(LS_REMOTE_SLEEP) + ';;\n'
                       '  add|commit|push|diff) exec ' + str(logger) + ' "$@";;\n'
                       'esac\n'
                       'exec ' + git_bin + ' "$@"\n')
    wrapper.chmod(0o700)

    def git(*argv, cwd):
        return subprocess.check_output([git_bin, *argv], cwd=cwd, env=env,
                                       stderr=subprocess.STDOUT, timeout=15)

    names = ('r1', 'r2', 'r3')
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
    seed_sha = subprocess.run([git_bin, '-C', str(repos['r3']), 'rev-parse', 'HEAD'],
                              capture_output=True, text=True, env=env, timeout=10).stdout.strip()

    policy = root / 'policy.toml'
    policy.write_text(
        'watch_roots = [' + json.dumps(str(watch)) + ']\n'
        'pulse_interval_secs = 1\ninactivity_push_delay_secs = 2\n'
        'auto_commit = true\nauto_push = true\nauto_pull = false\n'
        'auto_bump_versions = false\nauto_harden_with_warden = false\n'
        'auto_github_private = false\nauto_repair_concerns = false\n'
        'auto_repair_warns = false\nauto_rewrite_large_blobs = false\n'
        'build_artifact_cleanup = false\nstandard_files_auto = false\n'
        'auto_tag = false\nauto_release = false\nauto_publish = false\n'
        'auto_gc_garbage_threshold_bytes = 0\n'
        '[[remotes]]\nname = "origin"\n'
        'push_url = ' + json.dumps(str(root / 'remote-{repo}.git')) + '\n'
        'auto_create = true\n')
    env.update(DRACON_SYNC_POLICY=str(policy), DRACON_SYNC_GIT_BIN=str(wrapper),
               DRACON_SYNC_DEBUG='1')
    report = {'binary': binary, 'root': str(root), 'ls_remote_sleep': LS_REMOTE_SLEEP,
              'dispatch_budget_s': DISPATCH_BUDGET_S}

    def start_daemon(log_name):
        log = (root / log_name).open('w')
        proc = subprocess.Popen([binary, '-vv', 'daemon'], env=env, stdout=log,
                                stderr=subprocess.STDOUT, start_new_session=True)
        return proc, log

    def stop_daemon(proc, log):
        if proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=10)
        log.close()

    def remote_tip(repo):
        out = subprocess.run([git_bin, '-C', str(repo), 'ls-remote', 'origin', 'refs/heads/main'],
                             capture_output=True, text=True, env=env, timeout=15)
        return out.stdout.split()[0] if out.stdout.split() else None

    def wait_for(predicate, timeout, what):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return True
            time.sleep(0.25)
        raise RuntimeError(f'timeout waiting for {what}')

    # ── Phase A: seed dirty r3, require DAEMON convergence, stop ──
    (repos['r3'] / 'early.txt').write_text('written before first daemon start\n')

    def r3_daemon_converged():
        local = subprocess.run([git_bin, '-C', str(repos['r3']), 'rev-parse', 'HEAD'],
                               capture_output=True, text=True, env=env, timeout=10).stdout.strip()
        tip = remote_tip(repos['r3'])
        return bool(local) and local != seed_sha and tip == local

    daemon, log_a = start_daemon('daemon-phase-a.log')
    try:
        wait_for(lambda: r3_daemon_converged(), PHASE_A_TIMEOUT,
                 'phase-A daemon commit+push of r3')
        report['phase_a_converged'] = True
    finally:
        stop_daemon(daemon, log_a)

    # ── Phase B: second edit + restart, measure first dispatch of r3 ──
    (repos['r3'] / 'second.txt').write_text('edit after restart\n')
    daemon, log_b = start_daemon('daemon-phase-b.log')
    try:
        first_dispatch = None

        def scan():
            nonlocal first_dispatch
            text = (root / 'daemon-phase-b.log').read_text(errors='replace')
            for line in text.splitlines():
                if 'dispatch repo=' in line and str(repos['r3']) in line and 'unix_ms=' in line:
                    ms = int(re.search(r'unix_ms=(\d+)', line).group(1))
                    if first_dispatch is None or ms < first_dispatch:
                        first_dispatch = ms
            return first_dispatch is not None

        wait_for(scan, PHASE_B_TIMEOUT, 'phase-B dispatch of r3')
        pulse = None
        for line in (root / 'daemon-phase-b.log').read_text(errors='replace').splitlines():
            if 'pulse_start unix_ms=' in line:
                pulse = int(re.search(r'unix_ms=(\d+)', line).group(1))
                break
        latency_s = (first_dispatch - pulse) / 1000.0
        ls_remote_phase_b = 0
        if events.exists():
            for ev in events.read_text().splitlines():
                try:
                    e = json.loads(ev)
                except json.JSONDecodeError:
                    continue
                if e.get('args', [])[:1] == ['ls-remote'] and e.get('phase') == 'start':
                    ls_remote_phase_b += 1
        report.update({'phase_b_pulse_unix_ms': pulse, 'first_dispatch_unix_ms': first_dispatch,
                       'restart_dispatch_latency_s': round(latency_s, 3),
                       'ls_remote_calls_phase_b': ls_remote_phase_b,
                       'passed': latency_s <= DISPATCH_BUDGET_S})
    finally:
        stop_daemon(daemon, log_b)
    out_path.write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    return 0 if report.get('passed') else 1


if __name__ == '__main__':
    raise SystemExit(main())
