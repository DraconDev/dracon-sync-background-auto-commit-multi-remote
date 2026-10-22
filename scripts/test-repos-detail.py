#!/usr/bin/env python3
"""Behavioral CLI regressions; pass a built dracon-sync binary as argv[1].

All Git mutations are confined to a disposable fixture. A harmless extra repo
must not change detail results; a real status refusal must never become 'absent'.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
git = shutil.which("git")
assert git, "git is required"
with tempfile.TemporaryDirectory(prefix="sync-detail-test-") as temporary:
    root = Path(temporary)
    watch = root / "watch"
    watch.mkdir()
    for name in ["target", "unrelated"]:
        subprocess.run([git, "init", "--quiet", str(watch / name)], check=True)
    policy = root / "policy.toml"
    policy.write_text("watch_roots = " + json.dumps([str(watch)]) + "\n")
    calls = root / "calls"
    wrapper = root / "git-wrapper"
    wrapper.write_text(
        '#!/bin/sh\ncase " $* " in\n*" status "*)\n'
        '  printf "%s\\n" "$PWD" >> "$FIXTURE_CALLS"\n'
        '  if test "$FIXTURE_REFUSE" = yes; then\n'
        '    echo "fixture inspection refusal" >&2; exit 41\n'
        '  fi;;\nesac\nexec "$FIXTURE_GIT" "$@"\n'
    )
    wrapper.chmod(0o755)
    env = dict(os.environ, DRACON_SYNC_POLICY=str(policy),
               DRACON_SYNC_GIT_BIN=str(wrapper), FIXTURE_CALLS=str(calls),
               FIXTURE_GIT=git, FIXTURE_REFUSE="yes")
    result = subprocess.run([binary, "repos", "target"], env=env,
                            capture_output=True, text=True, timeout=90)
    output = result.stdout + result.stderr
    assert result.returncode != 0, output
    assert "status_failed" in output and "fixture inspection refusal" in output, output
    assert "not found in watch roots" not in output, output
    assert calls.read_text().splitlines() == [str(watch / "target")], output
    print("PASS: known inspection failure propagates; unrelated repo not inspected")
    # Change only the injected failure, not the repository or command.
    env["FIXTURE_REFUSE"] = "no"
    calls.write_text("")
    result = subprocess.run([binary, "repos", "target"], env=env,
                            capture_output=True, text=True, timeout=90)
    assert result.returncode == 0, result.stdout + result.stderr
    assert str(watch / "target") in result.stdout, result.stdout
    assert str(watch / "unrelated") not in result.stdout, result.stdout
    assert calls.read_text().splitlines() == [str(watch / "target")]
    print("PASS: successful detail unaffected by harmless extra repository")
