# dracon-sync v0.113.93 (2026-10-03)

ROUND3 audit remediation: 1 HIGH, 3 MEDIUM, 39 LOW findings fixed, plus the
unreleased ROUND2 audit batch and storage work already in the changelog.

- Nix watchdog scripts now come from the pinned utility inputs with
  `.source` existence assertions in `check-flake.sh`, and the checker
  asserts every shipped-unit property — Nix/HM drift fails CI.
- The bootstrap sweep sizes staged blobs fail-closed; auto-commit
  excludes are a global+per-repo union; detached-ahead count failures
  propagate; the stuck ledger is mutex-serialized with
  reload-before-save; stale Owned verdicts revalidate on TTL
  (two-strike flip); same-host visibility divergence aggregates to
  unknown; the dead settling knobs are removed (legacy keys ignored).
- Push errors join the fallback verdict with the loop cause,
  token-skipped legs say so, and all credential redaction runs through
  the one all-scheme redactor.
- Display: grapheme-width truncation, ACTIVITY widened with the 315-col
  floor documented, shortened durations, refreshed tier docs and
  re-mirrored width tests, compact A/B counts.

Also: installer reports success only after binaries land, fresh installs
`enable --now`, uninstall removes all watchdog units, and the freeze
watchdog no longer false auto-clears on a stat race.

Validation: 2189 workspace tests green, clippy/deny/fmt clean, both
deployment-checker suites pass, shellcheck clean, packaged-install
fixture check, live fleet at 0 stuck with JSON buckets summing exactly.

Install:

```bash
cargo install dracon-sync --version 0.113.93 --locked
```

[Full changelog](https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.92...dracon-sync-v0.113.93)
