#!/usr/bin/env bash
# scripts/resolve-github-remote.sh — print the name of the git remote whose
# URL points at github.com.
#
# v0.113.11: extracted from release.sh (which hardcoded the remote name
# `github` and failed identically on v0.113.9 + v0.113.10 because this
# repo names its github remote `origin`). Standalone so the derivation is
# directly testable against fixture repos.
#
# Usage:
#   scripts/resolve-github-remote.sh [repo-path]
#
# Resolution order:
#   1. Remote whose URL names the canonical dracon-sync repo.
#   2. Otherwise the first github.com remote (sorted); a warning goes to
#      stderr when there are several.
#
# Exit codes:
#   0  remote name printed on stdout
#   2  no github.com remote configured (loud message on stderr)
set -euo pipefail

REPO="${1:-$(git -C "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" rev-parse --show-toplevel)}"
CANON="dracon-sync-background-auto-commit-multi-remote"

names=()
while IFS= read -r line; do
    key="${line%% *}"
    url="${line#* }"
    case "${url,,}" in
        *github.com*) ;;
        *) continue ;;
    esac
    name="${key#remote.}"
    name="${name%.url}"
    names+=("$name")
done < <(git -C "$REPO" config --get-regexp '^remote\..*\.url$' || true)

if [[ ${#names[@]} -eq 0 ]]; then
    printf '✗ no github.com remote configured in %s; add one (git remote add <name> <github-url>) or pass --remote <name>\n' "$REPO" >&2
    exit 2
fi

# Prefer the canonical-repo URL.
for n in "${names[@]}"; do
    u="$(git -C "$REPO" config --get "remote.$n.url")"
    if [[ "$u" == *"$CANON"* ]]; then
        printf '%s\n' "$n"
        exit 0
    fi
done

# Otherwise: deterministic choice (sorted first), loud about ambiguity.
IFS=$'\n' sorted=($(printf '%s\n' "${names[@]}" | sort)); unset IFS
if [[ ${#sorted[@]} -gt 1 ]]; then
    printf '⚠ multiple github remotes (%s); using %s\n' "${sorted[*]}" "${sorted[0]}" >&2
fi
printf '%s\n' "${sorted[0]}"
