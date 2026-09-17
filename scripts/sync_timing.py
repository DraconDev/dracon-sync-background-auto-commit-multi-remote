"""Diagnostic timing decomposition; never combine unrelated clock origins."""
import re


def stage_timing(lines, repo, quiet_ms=2000):
    anchor = None
    dispatch = None
    stamps = {}
    for line in lines:
        match = re.search(r'scheduler: (\w+) repo=(\S+)(?: |$)', line)
        if not match or match[2] != str(repo):
            continue
        kind = match[1]
        fields = dict(re.findall(r'(\w+)=(-?\d+)(?= |$)', line))
        if kind == 'eligibility' and dispatch is None and 'anchor_daemon_ms' in fields:
            anchor = int(fields['anchor_daemon_ms'])
        if kind == 'dispatch' and dispatch is None:
            dispatch = fields
            if 'unix_ms' in fields:
                stamps[kind] = int(fields['unix_ms'])
        elif dispatch is not None and kind in ('task_start', 'stage_enter', 'add_spawn'):
            if 'unix_ms' in fields:
                stamps.setdefault(kind, int(fields['unix_ms']))
    if dispatch is None:
        return {}

    def delta(end, begin):
        if end not in stamps or begin not in stamps:
            return None
        return stamps[end] - stamps[begin]

    # Both operands relative to daemon epoch. Never subtract first-pulse time:
    # that pulse happens AFTER the daemon epoch, sometimes by hundreds of ms.
    dispatch_ms = int(dispatch['daemon_ms']) if 'daemon_ms' in dispatch else None
    return {
        'quiet_due_daemon_ms': None if anchor is None else anchor + quiet_ms,
        'dispatch_daemon_ms': dispatch_ms,
        'dispatch_after_quiet_ms': None if anchor is None or dispatch_ms is None
            else dispatch_ms - (anchor + quiet_ms),
        # All remaining deltas use timestamped daemon log events, in Unix ms.
        'dispatch_to_worker_start_ms': delta('task_start', 'dispatch'),
        'worker_start_to_stage_enter_ms': delta('stage_enter', 'task_start'),
        'stage_enter_to_add_spawn_ms': delta('add_spawn', 'stage_enter'),
        'dispatch_to_add_spawn_ms': delta('add_spawn', 'dispatch'),
    }
