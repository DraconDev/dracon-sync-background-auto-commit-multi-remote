#!/usr/bin/env python3
"""Timestamped 15-minute live observation of the installed dracon-sync daemon.

Read-only except one bounded synthetic append to the probe file, which must
be committed and pushed by the running daemon (no manual git).

Repairs vs the 0.113.63 observer (auditor findings 2026-09-17):
- Content-based probe identification: the daemon's commit subjects describe
  changed paths, so matching 'Round-5' in the subject never fires. This
  observer detects the commit whose TREE contains the unique marker line
  (`git show <rev>:<probe> | grep <marker>`), which is exactly the
  daemon-made commit regardless of subject wording.
- Timestamps are recorded AFTER each confirming read returns. A state
  observed by a read that completes at T proves the state held at or before
  T, so the post-read timestamp is a valid upper bound on completion.
  (The old observer stamped before network reads, which cannot bound
  completion.)
- All evidence is written under the repo audit directory, not /tmp.
"""
import json
import subprocess
import sys
import time
from pathlib import Path

DURATION = 15 * 60
POLL = 5.0
SCRIPT_DIR = Path(__file__).resolve().parent
REPO = SCRIPT_DIR.parent
PROBE_REL = Path('audit/live-convergence-probe-2026-09-17b.md')
OUT_DIR = REPO / 'audit' / 'sync-convergence-repair-evidence' / 'live-observation-0.113.64'
BIN = Path.home() / '.local/bin/dracon-sync'


def run(args, timeout=60):
    return subprocess.run(args, capture_output=True, text=True, timeout=timeout)


def head():
    return run(['git', '-C', str(REPO), 'rev-parse', 'HEAD']).stdout.strip()


def tree_contains_marker(rev, marker):
    r = run(['git', '-C', str(REPO), 'show', f'{rev}:{PROBE_REL}'])
    return r.returncode == 0 and marker in r.stdout


def commit_with_marker(marker):
    """Newest commit whose tree contains the marker (empty string if none)."""
    r = run(['git', '-C', str(REPO), 'log', '--format=%H', '-S', marker,
             '--', str(PROBE_REL)])
    lines = [l for l in r.stdout.splitlines() if l.strip()]
    return lines[0] if lines else ''


def remote_tip(remote):
    r = run(['git', '-C', str(REPO), 'ls-remote', remote, 'refs/heads/main'], timeout=45)
    return r.stdout.split()[0] if r.stdout.strip() else None


def inventory():
    r = run([str(BIN), 'repos', '--json'], timeout=120)
    if r.returncode != 0:
        return {'error': r.stderr[-300:]}
    try:
        data = json.loads(r.stdout)
    except json.JSONDecodeError:
        return {'error': 'repos --json not JSON'}
    rows = []
    for row in data.get('rows', []):
        rows.append({
            'repo': row.get('repo'), 'state': row.get('state_cause'),
            'dirty': row.get('modified', 0) + row.get('staged', 0) + row.get('untracked', 0),
            'excluded_dirty': row.get('excluded_dirty', 0),
            'ahead': row.get('ahead', 0), 'behind': row.get('behind', 0),
            'push': row.get('push_status'), 'push_error': row.get('push_error', ''),
            'remotes': row.get('push_to_remotes', []),
        })
    return {'row_count': len(rows), 'rows': rows}


def journal_counts(since_epoch):
    r = run(['journalctl', '--user', '-u', 'dracon-sync.service',
             '--since', '@' + str(int(since_epoch)), '--no-pager'], timeout=45)
    lines = r.stdout.splitlines()
    return {'committed': sum(1 for l in lines if 'committed' in l),
            'synced': sum(1 for l in lines if 'synced' in l)}


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    start_iso = time.strftime('%Y-%m-%dT%H:%M:%S%z')
    marker = f'PROBE-MARKER-0.113.64-{int(time.time())}'
    probe = REPO / PROBE_REL
    baseline = probe.read_text()
    probe.write_text(baseline + f'\n{marker} live window opened {start_iso}.\n')
    written = time.time()
    report = {'start_iso': start_iso, 'daemon': 'installed ~/.local/bin/dracon-sync',
              'duration_s': DURATION, 'marker': marker,
              'probe_written_unix': written,
              'snapshots': [], 'probe_events': [], 'inventories': []}
    report['inventories'].append({'observed_at_unix': time.time(), 'phase': 'baseline',
                                  'inventory': inventory()})
    seen_commit = seen_gh = seen_gl = None
    commit_rev = ''
    last_inventory = time.time()
    start = time.time()
    while time.time() - start < DURATION:
        time.sleep(POLL)
        if seen_commit is None:
            rev = commit_with_marker(marker)
            if rev and tree_contains_marker(rev, marker):
                seen_commit = time.time()  # AFTER the confirming reads
                commit_rev = rev
                report['probe_events'].append(
                    {'event': 'daemon_commit', 'at_unix': seen_commit,
                     'upper_bound_after_write_s': round(seen_commit - written, 1),
                     'rev': rev[:12]})
        gh = remote_tip('origin')
        t_after_gh = time.time()
        gl = remote_tip('gitlab')
        t_after_gl = time.time()
        if seen_commit and seen_gh is None and gh == commit_rev:
            seen_gh = t_after_gh
            report['probe_events'].append(
                {'event': 'push_github', 'at_unix': seen_gh,
                 'upper_bound_after_write_s': round(seen_gh - written, 1)})
        if seen_commit and seen_gl is None and gl == commit_rev:
            seen_gl = t_after_gl
            report['probe_events'].append(
                {'event': 'push_gitlab', 'at_unix': seen_gl,
                 'upper_bound_after_write_s': round(seen_gl - written, 1)})
        now = time.time()
        snap = {'observed_at_unix': now,
                'committed_ub_after_write_s': None if seen_commit is None else round(seen_commit - written, 1),
                'pushed_github_ub_after_write_s': None if seen_gh is None else round(seen_gh - written, 1),
                'pushed_gitlab_ub_after_write_s': None if seen_gl is None else round(seen_gl - written, 1),
                'commit_rev': commit_rev[:12], 'github_tip': (gh or '')[:12],
                'gitlab_tip': (gl or '')[:12], 'journal': journal_counts(start)}
        report['snapshots'].append(snap)
        if now - last_inventory >= 60:
            last_inventory = now
            report['inventories'].append({'observed_at_unix': now, 'phase': 'minute',
                                          'inventory': inventory()})
        (OUT_DIR / 'live-report.json').write_text(json.dumps(report, indent=2))
        print(f"t={round(now-start,1)} commit={snap['committed_ub_after_write_s']} "
              f"gh={snap['pushed_github_ub_after_write_s']} gl={snap['pushed_gitlab_ub_after_write_s']}",
              flush=True)
    report['inventories'].append({'observed_at_unix': time.time(), 'phase': 'final',
                                  'inventory': inventory()})
    final = {'committed_ub_after_write_s': report['snapshots'][-1]['committed_ub_after_write_s'],
             'pushed_github_ub_after_write_s': report['snapshots'][-1]['pushed_github_ub_after_write_s'],
             'pushed_gitlab_ub_after_write_s': report['snapshots'][-1]['pushed_gitlab_ub_after_write_s'],
             'probe_events': report['probe_events']}
    (OUT_DIR / 'final.json').write_text(json.dumps(final, indent=2))
    (OUT_DIR / 'live-report.json').write_text(json.dumps(report, indent=2))
    print('FINAL', json.dumps(final))
    return 0 if (seen_commit and seen_gh and seen_gl) else 1


if __name__ == '__main__':
    raise SystemExit(main())
