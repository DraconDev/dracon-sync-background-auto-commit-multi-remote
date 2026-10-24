"""Select historical eligible work without requiring it to survive later edits."""


def select_witness(repo, witness, journal, daemon_pid, git, ancestor):
    result = {
        'witness_commit': '', 'actual_blob': None, 'commit_metadata': [],
        'object_and_advancement_verified': False,
        'matching_loaded_daemon_commit_events': [],
        'candidate_commits_considered': [],
    }
    # Git timestamps precede commit hooks; correlate through the recorded
    # effective finite commit-operation budget, not an incidental 30-second
    # assumption. Older receipts retain their original 30-second bound.
    budget = witness.get('commit_event_budget_secs', 30)
    if isinstance(budget, bool) or not isinstance(budget, int) or budget <= 0:
        return result
    result['commit_event_budget_secs'] = budget
    history = git(repo, 'log', '--format=%H', 'HEAD', '--', witness['eligible_path'])
    if history.returncode:
        return result
    for commit in history.stdout.splitlines():
        result['candidate_commits_considered'].append(commit)
        blob = git(repo, 'rev-parse', '--verify', commit + ':' + witness['eligible_path'])
        # A later deletion/rename has no blob at this path. Never mistake
        # rev-parse's failed-expression output for an actual object ID.
        if blob.returncode or blob.stdout.strip() != witness['eligible_blob']:
            continue
        metadata = git(repo, 'show', '-s', '--format=%H%n%P%n%ct%n%an%n%ae', commit)
        if metadata.returncode:
            continue
        fields = metadata.stdout.strip().splitlines()
        if len(fields) != 5:
            continue
        try:
            timestamp = int(fields[2])
        except ValueError:
            continue
        kind = git(repo, 'cat-file', '-t', commit)
        object_ok = (
            fields[0] == commit and kind.returncode == 0
            and kind.stdout.strip() == 'commit'
            and commit != witness['before_head']
            and ancestor(repo, witness['before_head'], commit)
            and timestamp >= int(witness['before_observed_at'])
            and timestamp + 1 >= witness['file_mtime']
        )
        if not object_ok:
            continue
        events = []
        for event in journal:
            try:
                stamp = int(event.get('__REALTIME_TIMESTAMP', 0)) / 1_000_000
            except (ValueError, TypeError):
                continue
            message = event.get('MESSAGE', '')
            if (str(event.get('_PID')) == str(daemon_pid)
                    and isinstance(message, str)
                    and message.startswith('📝 committed ')
                    and message.endswith(' in ' + repo)
                    and timestamp <= stamp <= timestamp + budget):
                events.append({'timestamp_us': event['__REALTIME_TIMESTAMP'],
                               'pid': event['_PID'], 'message': message})
        if not events:
            continue
        result.update(witness_commit=commit, actual_blob=blob.stdout.strip(),
                      commit_metadata=fields, object_and_advancement_verified=True,
                      matching_loaded_daemon_commit_events=events)
        return result
    return result
