#!/usr/bin/env python3
"""close-changelog.py — close CHANGELOG.md's [Unreleased] section as [VERSION].

v0.113.11: extracted from release.sh's inline heredoc so the idempotency
property is directly testable. Idempotent: when a `## [VERSION]` header
already exists, the file is left BYTE-IDENTICAL (the v0.113.10 re-run
after a partial failure duplicated the header; this is the fix).

Usage: close-changelog.py <changelog-path> <version> <date>
Exit 0 always (a missing [Unreleased] or an already-closed version is a
no-op with a message on stderr, not an error).
"""
import pathlib
import re
import sys


def main() -> int:
    path, version, date = sys.argv[1], sys.argv[2], sys.argv[3]
    p = pathlib.Path(path)
    text = p.read_text()

    # Idempotency: this version is already closed -> leave byte-identical.
    if re.search(rf"^## \[{re.escape(version)}\][^\n]*$", text, re.MULTILINE):
        print(
            f"  {p}: [{version}] already closed; leaving unchanged",
            file=sys.stderr,
        )
        return 0

    marker = "## [Unreleased]"
    if marker not in text:
        print(
            f"  {p}: no [Unreleased] section found; leaving unchanged",
            file=sys.stderr,
        )
        return 0

    unreleased_match = re.search(r"^## \[Unreleased\][^\n]*\n", text, re.MULTILINE)
    if not unreleased_match:
        print(f"  {p}: regex miss for [Unreleased] header; leaving unchanged", file=sys.stderr)
        return 0

    start = unreleased_match.end()
    next_match = re.search(r"^## \[[^\n]*\n", text[start:], re.MULTILINE)
    if next_match:
        end = start + next_match.start()
        new_header = f"## [{version}] - {date}\n"
        new_text = text[:start] + new_header + text[start:end] + text[end:]
    else:
        new_text = text[:start] + f"\n## [{version}] - {date}\n" + text[start:]
    p.write_text(new_text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
