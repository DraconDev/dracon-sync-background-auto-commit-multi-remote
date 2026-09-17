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
import re
from pathlib import Path
import shutil
import shlex
import signal
import subprocess
import sys
import tempfile
import threading
import time
from sync_timing import stage_timing

DEADLINE = 30.0


def main():
    binary = str(Path(sys.argv[1]).resolve())
    slow_filter = '--slow-filter' in sys.argv[2:]
    failing_remote = '--failing-remote' in sys.argv[2:]
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
    # A daemon that never reaches Git should fail timing checks, not crash
    # while opening a log that the first Git subprocess has not created yet.
    events.touch()
    wrapper = root / 'git-probe'
    logger = root / 'git-event-probe'
    # C's push is slowed by 3s inside the wrapper: execution/transfer cost is
    # visible separately from queue wait, without touching any timeout config.
    logger.write_text('#!/usr/bin/env python3\nimport json,os,subprocess,sys,time\n'
                       + 'def event(phase, **extra):\n'
                       + f' with open({str(events)!r}, "a") as f:\n'
                       + '  f.write(json.dumps({"t":time.monotonic(),"args":sys.argv[1:],'
                       '"cwd":os.getcwd(),"pid":os.getpid(),"phase":phase,**extra})+"\\n")\n'
                       + 'event("start")\n'
                       + 'if sys.argv[1:2] == ["push"] and "--delete" not in sys.argv and os.getcwd().endswith("/c"):\n'
                       + ' time.sleep(3)\n'
                       + 'if sys.argv[1:2] == ["push"] and "--delete" not in sys.argv and os.getcwd().endswith("/f-failing"):\n'
                       + ' time.sleep(5)\n event("end", returncode=1)\n sys.exit(1)\n'
                       + f'rc=subprocess.call([{git_bin!r}]+sys.argv[1:])\n'
                       + 'event("end", returncode=rc)\nsys.exit(rc)\n')
    logger.chmod(0o700)
    # Only instrument operations under test, not every status/config probe.
    wrapper.write_text('#!' + shutil.which('sh') + '\n'
                       + 'case "$1" in add|commit|push|diff) exec ' + shlex.quote(str(logger)) + ' "$@";; esac\n'
                       + 'exec ' + shlex.quote(git_bin) + ' "$@"\n')
    wrapper.chmod(0o700)

    def git(*argv, cwd):
        return subprocess.check_output([git_bin, *argv], cwd=cwd, env=env,
                                       stderr=subprocess.STDOUT, timeout=10)

    names = ('a', 'b', 'b2', 'c', 'd-filter') if slow_filter else ('a', 'b', 'b2', 'c')
    if failing_remote:
        names += ('f-failing',)
    healthy_names = tuple(n for n in names if n != 'f-failing')
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
        filter_program.write_text('import json,os,sys,time\n'
                                  'def event(phase):\n'
                                  + f' with open({str(root / "filter-events.jsonl")!r}, "a") as f:\n'
                                  + '  f.write(json.dumps({"t":time.monotonic(),"pid":os.getpid(),"phase":phase})+"\\n")\n'
                                  'event("start")\ntime.sleep(4)\n'
                                  'sys.stdout.buffer.write(sys.stdin.buffer.read())\n'
                                  'sys.stdout.buffer.flush()\nevent("end")\n')
        filtered_repo = repos['d-filter']
        git('config', 'filter.probe.clean',
            shlex.quote(sys.executable) + ' ' + shlex.quote(str(filter_program)), cwd=filtered_repo)
        git('config', 'filter.probe.required', 'true', cwd=filtered_repo)
        # info/attributes is fixture-local and need not be committed.
        (filtered_repo / '.git/info/attributes').write_text('seed.txt filter=probe\n')
        (filtered_repo / 'seed.txt').write_text('filtered change\n')
    if failing_remote:
        (repos['f-failing'] / 'seed.txt').write_text('failed remote change\n')
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
              'slow_required_filter': slow_filter, 'failing_remote': failing_remote}
    log = (root / 'daemon.log').open('w')
    daemon = subprocess.Popen([binary, '-vv', 'daemon'], env=env, stdout=log,
                              stderr=subprocess.STDOUT, start_new_session=True)
    stop_editing = threading.Event()
    editor = None
    try:
        start = time.monotonic()
        start_unix = time.time()
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
            failure_observed = not failing_remote or any(
                row.get('returncode') == 1 and row['cwd'] == str(repos['f-failing'])
                and row['args'][:1] == ['push'] and '--delete' not in row['args']
                for row in (json.loads(line) for line in events.read_text().splitlines()))
            if len(seen) == len(healthy_names) and failure_observed:
                break
            time.sleep(0.05)
        stop_editing.set()
        editor.join(timeout=2)
        report['converged'] = len(seen) == len(healthy_names)
        report['commit_observed_seconds'] = {n: t - start for n, t in committed.items()}
        report['remote_observed_seconds'] = {n: t - start for n, t in seen.items()}
        report['a_commits'] = int(git('rev-list', '--count', 'HEAD', cwd=repos['a'])) - 1
        rows = [json.loads(line) for line in events.read_text().splitlines()]
        dispatch = {}

        def first(name, op, after=0.0):
            for row in rows:
                if row.get('phase', 'start') == 'start' and row['t'] >= after and row['cwd'] == str(repos[name]) \
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
        # b2 is dirty BEFORE the daemon starts (missed-event reconciliation).
        # The quiet clock cannot anchor before launch: measure dispatch from
        # the daemon's own first-observation anchor, plus quiet + one pulse.
        b2_anchor = None
        b2_dispatch_ms = None
        for line in (root / 'daemon.log').read_text().splitlines():
            if '/watch/b2 ' not in line:
                continue
            if b2_anchor is None and 'scheduler: eligibility repo=' in line:
                m = re.search(r'anchor_daemon_ms=(\d+)', line)
                if m:
                    b2_anchor = int(m.group(1))
            elif 'scheduler: dispatch repo=' in line:
                m = re.search(r'daemon_ms=(\d+)', line)
                n = re.search(r'inspection_ms=(\d+)', line)
                if m:
                    b2_dispatch_ms = int(m.group(1))
                    if n:
                        b2_dispatch_ms -= int(n.group(1))
                    break
        report['dispatch'] = dispatch
        report['edit_seconds'] = {'b': t_b - start, 'c': t_c - start,
                                  'a_started': t_a - start}
        # Compare actual command boundaries, not the later HEAD observation.
        # Pair by wrapper PID so unrelated commits cannot satisfy the gate.
        commit_ends = {}
        for row in rows:
            if (row.get('phase') == 'end' and row.get('returncode') == 0
                    and row['args'][:1] == ['commit']):
                for name, repo in repos.items():
                    if row['cwd'] == str(repo):
                        commit_ends.setdefault(name, row['t'])
        report['commit_command_end_seconds'] = {
            name: t - start for name, t in commit_ends.items()}
        report['edit_to_add_seconds'] = {
            name: None if dispatch[name]['add_rel'] is None
            else start + dispatch[name]['add_rel'] - edited
            for name, edited in [('b', t_b), ('c', t_c)]}
        # Queue delay from the DAEMON's own monotonic timestamps: the
        # dispatching cycle's start vs quiet expiry (anchor + quiet window),
        # and the eligibility decision must confirm quiet had expired. This
        # separates queue wait from in-cycle inspection/execution cost, which
        # the contract says to measure separately. Parsed from daemon.log
        # debug lines. Also decomposes the staging miss: dispatch-vs-due
        # (scheduler decision) and add-vs-dispatch (worker execution), so a
        # load-starved worker is visible as execution cost, not queue delay.
        queue_delay = {}
        stage_decomposition = {}
        daemon_log_lines = (root / 'daemon.log').read_text().splitlines()
        # stage_timing pairs timestamps only within the same clock domain.
        for name in names:
            anchor = None
            decision_ms = None
            eligible = False
            for line in daemon_log_lines:
                if f'/watch/{name} ' not in line:
                    continue
                if 'scheduler: eligibility repo=' in line:
                    m = re.search(r'anchor_daemon_ms=(\d+)', line)
                    d = re.search(r'daemon_ms=(\d+)', line)
                    if m and d:
                        anchor = int(m.group(1))
                        candidate = int(d.group(1))
                        if 'eligible=true' in line:
                            decision_ms = candidate
                            eligible = True
                            break
            if eligible:
                m = re.search(
                    rf'scheduler: dispatch repo=.*watch/{re.escape(name)} '
                    r'.*cycle_ms=(\d+)',
                    '\n'.join(daemon_log_lines),
                )
                cycle_ms = int(m.group(1)) if m else 0
                if decision_ms is not None:
                    queue_delay[name] = decision_ms - cycle_ms - (anchor + 2000)
            stage_decomposition[name] = stage_timing(daemon_log_lines, repos[name])
        report['queue_delay_ms'] = queue_delay
        report['stage_decomposition_ms'] = stage_decomposition
        # Load context: a staging miss whose dispatch fired on time but whose
        # add landed late is worker-execution starvation, not a queue defect.
        # Record load so the report distinguishes the two without a reroll.
        try:
            with open('/proc/loadavg') as f:
                fields = f.read().split()
            report['loadavg_end'] = [float(fields[0]), float(fields[1]), float(fields[2])]
        except OSError:
            report['loadavg_end'] = None
        # Exact commit completion from the daemon's own timestamped line
        # (GitService::commit is in-process libgit2; the CLI wrapper cannot
        # observe it). Push start comes from the wrapper's unix timestamp.
        commit_done_unix_ms = {}
        for line in (root / 'daemon.log').read_text().splitlines():
            if 'scheduler: commit_done repo=' not in line:
                continue
            m = re.search(r'repo=(\S+) unix_ms=(\d+)', line)
            if m:
                commit_done_unix_ms.setdefault(m.group(1), int(m.group(2)))
        report['commit_done_unix_ms'] = commit_done_unix_ms
        checks = {
            # Acceptance measures actual staging start, not an earlier cycle
            # start. Keep internal queue estimates diagnostic only: subtracting
            # inspection time must not hide a missed staging deadline.
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
            # Exact gate: push start within one pulse of the daemon's own
            # commit-completion timestamp (unix ms on both sides).
            # Exact gate: push start within one pulse of the daemon's own
            # commit-completion timestamp (unix ms on both sides). Wrapper
            # spawn may precede the log line by a few ms; small negative
            # slack avoids false failures from clock-read ordering.
            'b_push_within_one_pulse_of_commit_done': b_push is not None
                and str(repos['b']) in commit_done_unix_ms
                and -100 <= (start_unix + b_push) * 1000
                    - commit_done_unix_ms[str(repos['b'])] <= 1000,
            'continuous_work_reaches_remote_within_10s': 'a' in seen and seen['a'] - t_a <= 10,
            # Reconciliation must include initial scan time, rather than
            # resetting the acceptance clock when the daemon notices the file.
            'pre_start_change_stages_within_3s': b2_add is not None and b2_add <= 3,
        }
        if slow_filter:
            checks['slow_required_filter_eventually_converges'] = 'd-filter' in seen
        if failing_remote:
            failures = [row for row in rows if row['cwd'] == str(repos['f-failing'])
                        and row['args'][:1] == ['push'] and '--delete' not in row['args']
                        and row.get('returncode') == 1]
            failed_start = first('f-failing', 'push')
            checks['failing_remote_attempt_observed'] = bool(failures)
            checks['healthy_staging_during_failing_push'] = (
                failed_start is not None and b_add is not None and bool(failures)
                and failed_start <= start + b_add <= failures[0]['t'])
            checks['failed_remote_does_not_claim_convergence'] = (
                git('--git-dir', str(root / 'remote-f-failing.git'), 'show',
                    'refs/heads/main:seed.txt', cwd=root) == b'seed\n')
            report['failing_push_events'] = [row for row in rows
                if row['cwd'] == str(repos['f-failing']) and row['args'][:1] == ['push']]
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
