#!/usr/bin/env python3
"""Isolated pre-push dispatcher primitive; NOT installed or used by Sync/Warden.

Trusted callers supply executable guard/transfer commands. Guards must implement
all Warden/history checks plus repo/path/OID-bound payload authorization. This
module neither implements that authorization nor certifies stock LFS safety.
No CLI/install entrypoint is intentionally provided until integration review.
"""

from dataclasses import dataclass
import os
from pathlib import Path
import re
import signal
import subprocess
import time
from typing import Sequence

MAX_REFS_BYTES = 1024 * 1024
_REENTRANT = "DRACON_LFS_DISPATCH_ACTIVE"
_OID = re.compile(rb"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")


@dataclass(frozen=True)
class Result:
    """Separate phase attribution without exposing command args or credentials."""

    phase: str
    code: int


def _valid_refs(refs: bytes) -> bool:
    if len(refs) > MAX_REFS_BYTES or b"\x00" in refs:
        return False
    if not refs:
        return True
    if not refs.endswith(b"\n"):
        return False
    for line in refs.splitlines():
        fields = line.split(b" ")
        if len(fields) != 4:
            return False
        local, local_oid, remote, remote_oid = fields
        if not local or not remote or any(c in local + remote for c in b"\t\r"):
            return False
        if not _OID.fullmatch(local_oid) or not _OID.fullmatch(remote_oid):
            return False
        if len(local_oid) != len(remote_oid):
            return False
    return True


def _valid_commands(commands: Sequence[tuple[str, ...]]) -> bool:
    return all(
        command
        and Path(command[0]).is_absolute()
        and Path(command[0]).is_file()
        and os.access(command[0], os.X_OK)
        for command in commands
    )


def _invoke(command: tuple[str, ...], push_args: tuple[str, str], refs: bytes,
            env: dict[str, str], deadline: float) -> int:
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        return 124
    try:
        child = subprocess.Popen((*command, *push_args), stdin=subprocess.PIPE,
                                 env=env, start_new_session=True)
    except OSError:
        return 126
    try:
        child.communicate(input=refs, timeout=remaining)
        return child.returncode if child.returncode >= 0 else 128 - child.returncode
    except subprocess.TimeoutExpired:
        return 124
    finally:
        # Also stop residual descendants. A guard must finish all authorization
        # before returning; asynchronous transfer outside this lifecycle is invalid.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait()


def dispatch(*, guards: Sequence[tuple[str, ...]], transfers: Sequence[tuple[str, ...]],
             push_args: tuple[str, str], refs: bytes, timeout_seconds: float = 30,
             env: dict[str, str] | None = None) -> Result:
    """Run every required guard before any transfer, replaying exact args/stdin.

    Failure stops this invocation. No retry, raw fallback, Git push, attribute
    mutation or global-hook installation occurs. Approved executable paths and
    immutable authorization evidence remain obligations of the future adapter.
    A single deadline bounds the whole sequence, not a fresh budget per hook.
    """
    environment = dict(os.environ if env is None else env)
    # Snapshot mutable caller sequences before validation/execution.
    guard_commands = tuple(tuple(c) for c in guards)
    transfer_commands = tuple(tuple(c) for c in transfers)
    arguments = tuple(push_args)
    if environment.get(_REENTRANT) is not None:
        return Result("recursion", 1)
    if (not guard_commands or len(arguments) != 2 or not _valid_refs(refs)
            or not 0 < timeout_seconds <= 300
            or not _valid_commands((*guard_commands, *transfer_commands))):
        return Result("configuration", 1)
    environment[_REENTRANT] = "1"
    deadline = time.monotonic() + timeout_seconds
    for phase, commands in (("security", guard_commands), ("transfer", transfer_commands)):
        for command in commands:
            code = _invoke(command, arguments, refs, environment, deadline)
            if code:
                return Result(phase, code)
    return Result("ready-for-git", 0)
