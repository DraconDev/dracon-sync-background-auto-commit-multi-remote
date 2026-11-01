#!/usr/bin/env python3
"""Mutation checks in a private, independent workspace/target; never patch live input.

Run from any cwd. The owned directory and all failed/timeout artifacts are retained.
A nonzero result is NOT successful non-vacuity unless the selected behavioral
assertion failed with Rust's test-failure exit status. Cargo/timeout errors stop.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import tempfile
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot(source, destination):
    observations = []
    for relative in [Path('.'), Path('dracon-sync'), Path('dracon-system'), Path('dracon-warden')]:
        repo = source / relative
        files = subprocess.check_output(['git', '-C', str(repo), 'ls-files', '--cached', '-z']).split(b'\0')
        for raw in files:
            if not raw:
                continue
            name = Path(os.fsdecode(raw))
            if any(part in {'.git', '.pi-glla', '.pi', 'target', 'node_modules'} for part in name.parts):
                continue
            original = repo / name
            target = destination / relative / name
            before = original.lstat()
            target.parent.mkdir(parents=True, exist_ok=True)
            if stat.S_ISLNK(before.st_mode):
                os.symlink(os.readlink(original), target)
                observations.append({'path': str(relative / name), 'symlink': os.readlink(original)})
            else:
                if not stat.S_ISREG(before.st_mode):
                    raise RuntimeError(f'unsupported tracked input: {original}')
                shutil.copy2(original, target)
                after = original.stat()
                if (before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != \
                   (after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                    raise RuntimeError(f'input changed during snapshot: {original}')
                observations.append({'path': str(relative / name), 'sha256': digest(target)})
    # Compare the complete copied source scope before compiling any mutation.
    for row in observations:
        path = source / row['path']
        if 'sha256' in row and digest(path) != row['sha256']:
            raise RuntimeError(f'input changed before snapshot completed: {path}')
        if 'symlink' in row and os.readlink(path) != row['symlink']:
            raise RuntimeError(f'symlink changed before snapshot completed: {path}')
    return observations


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError(f'mutation anchor not unique: {old!r}')
    return text.replace(old, new, 1)


def mutate_duplicate(text):
    return replace_once(text, 'if fields[1] == "missing" {', 'if true { // MUTATION: classify all source objects as main-missing')


def mutate_rescue(text):
    start = text.index('    let listing = work.join("unique-listing");')
    end = text.index('    Ok(Payload {', start)
    # Corruption still happens after write/index, but the entire independent
    # pack/enumeration/content readback boundary is deliberately skipped.
    return text[:start] + ('    // MUTATION: accept the written sidecar without verification.\n'
        '    let uncompressed_bytes = 0u64;\n'
        '    fs::write(record.join("unique-readback.tsv"), b"")?;\n') + text[end:]


def mutate_ownership(text):
    text = replace_once(text, '        file.try_lock().context("spool is still owned")?;',
        '        // MUTATION: ignore the live producer/reclaimer kernel fence.')
    start = text.index('    pub(super) fn is_settled(&self) -> bool {')
    end = text.index('    pub(super) fn still_owned(&self) -> bool {', start)
    # Marker and lock are independent defenses in the same ownership boundary.
    # Bypass BOTH explicitly; bypassing just one must not be called proof that
    # the other is vacuous. Inode/path/preserve/data checks remain unchanged.
    return text[:start] + ('    pub(super) fn is_settled(&self) -> bool {\n'
        '        true // MUTATION: treat a live, unsealed producer as settled.\n'
        '    }\n\n') + text[end:]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--workspace', type=Path, default=Path(__file__).resolve().parents[4])
    parser.add_argument('--cache', type=Path, default=Path.home() / '.cache/space-hygiene')
    parser.add_argument('--command-timeout', type=int, default=900)
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    owned = Path(tempfile.mkdtemp(prefix='epoch-corrected-nonvacuity-', dir=args.cache))
    workspace = owned / 'workspace'
    workspace.mkdir()
    report = owned / 'report'
    report.mkdir()
    print(f'OWNED_DIRECTORY={owned}', flush=True)
    source = args.workspace.resolve()
    inputs = snapshot(source, workspace)
    (report / 'snapshot-inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')
    environment = dict(os.environ)
    environment['CARGO_TARGET_DIR'] = str(owned / 'target')
    environment['CARGO_BUILD_JOBS'] = '4'
    environment['DRACON_EPOCH_TEST_HELPER'] = str(owned / 'target/debug/dracon-sync')
    results = []

    def run(label, command, expected_failure=None, expected_test=None):
        path = report / (label + '.log')
        started = time.monotonic()
        with path.open('wb') as output:
            process = subprocess.Popen(command, cwd=workspace, env=environment,
                stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = process.wait(timeout=args.command_timeout)
            except subprocess.TimeoutExpired:
                # Only this explicitly owned, unreaped process group is killed.
                # Do not remove its storage or touch unrelated build processes.
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                code = 124
        text = path.read_text(errors='replace')
        row = {'label': label, 'command': command, 'exit': code,
            'duration_seconds': round(time.monotonic() - started, 3), 'log': path.name}
        results.append(row)
        (report / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
        print(json.dumps(row), flush=True)
        if expected_failure:
            if code != 101 or expected_failure not in text or f'test {expected_test} ... FAILED' not in text:
                raise RuntimeError(f'{label}: no discriminating behavioral failure; inspect {path}')
        else:
            if code != 0:
                raise RuntimeError(f'{label}: failed/incomplete command; inspect {path}')
            if expected_test and f'test {expected_test} ... ok' not in text:
                raise RuntimeError(f'{label}: selected test did not demonstrably run')

    cases = [
        ('duplicate', 'epoch_spool_unique.rs', mutate_duplicate,
         'git::epoch_spool::tests::duplicate_spool_is_reaped',
         'classification must distinguish duplicates from mixed payloads'),
        ('rescue-verification', 'epoch_spool_unique.rs', mutate_rescue,
         'git::epoch_spool::tests::post_write_rescue_verification_failure_preserves_original',
         'post-write corruption must fail verification, not grant deletion'),
        ('ownership', 'epoch_spool_ownership.rs', mutate_ownership,
         'git::epoch_spool::tests::old_but_live_owner_cannot_be_reclaimed',
         'assertion failed: attempt(&f, reclaim::Fault::None).await.is_err()'),
    ]
    for label, filename, mutation, test, assertion in cases:
        path = workspace / 'dracon-sync/src/git' / filename
        if path.is_symlink():
            raise RuntimeError('refusing to mutate a symlink')
        original = path.read_bytes()
        command = ['cargo', 'test', '--locked', '-p', 'dracon-sync', test, '--', '--exact']
        run(label + '-baseline', command, expected_test=test)
        try:
            changed = mutation(original.decode()).encode()
            path.write_bytes(changed)
            (report / (label + '-mutation.patch')).write_bytes(subprocess.check_output(
                ['diff', '-u', '--label', 'original/' + filename, '--label', 'mutated/' + filename,
                 str(source / 'dracon-sync/src/git' / filename), str(path)])
                if False else b'')
            # diff's exit 1 means differences, not an operational failure.
            diff = subprocess.run(['diff', '-u', '--label', 'original/' + filename,
                '--label', 'mutated/' + filename,
                str(source / 'dracon-sync/src/git' / filename), str(path)], capture_output=True)
            if diff.returncode != 1:
                raise RuntimeError('mutation diff missing or failed')
            (report / (label + '-mutation.patch')).write_bytes(diff.stdout)
            run(label + '-broken', command, expected_failure=assertion, expected_test=test)
        finally:
            path.write_bytes(original)
        run(label + '-restored', command, expected_test=test)
        if path.read_bytes() != original:
            raise RuntimeError('scratch source restoration failed')
    run('restored-helper-build', ['cargo', 'build', '--locked', '-p', 'dracon-sync', '--bin', 'dracon-sync'])
    run('restored-epoch-spool', ['cargo', 'test', '--locked', '-p', 'dracon-sync', 'epoch_spool', '--', '--include-ignored'])
    # The live files are read-only inputs throughout; assert mutation targets did
    # not drift while demonstrating the copied regressions.
    captured = {row['path']: row.get('sha256') for row in inputs}
    for filename in {'epoch_spool_unique.rs', 'epoch_spool_ownership.rs'}:
        relative = 'dracon-sync/src/git/' + filename
        if digest(source / relative) != captured[relative]:
            raise RuntimeError('live input changed; do not treat this as current-tree evidence')
    (report / 'PASS').write_text('All three safety mutations failed their named behavioral assertions; each restored case and the restored complete spool suite passed.\n')
    print(f'REPORT={report}', flush=True)


if __name__ == '__main__':
    main()
