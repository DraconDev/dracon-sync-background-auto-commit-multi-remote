# dracon-sync v0.113.92 (2026-10-01)

Fixes stalled automatic commits and canonical author display.

- Classification retries now wait for the cooldown deadline and resume
  after expiry. This fixes both retry storms and quiet permanent stalls.
- TOUCHED respects Git mailmaps so confirmed author aliases display one
  canonical name without rewriting commits or changing activity times.
- Releases retain the verified package lockfile for standalone locked builds.

The live repair also standardized all 35 watched repositories to
`DraconDev <dracsharp@gmail.com>` and restarted the music synchronizer with
its bounded catalog writer. Those are operator configuration changes, not
new daemon defaults.

Validation includes the cooldown boundary test, a real Git mailmap fixture,
the release lockfile regression, workspace gates, and packaged-install checks.

Install:

```bash
cargo install dracon-sync --version 0.113.92 --locked
```

[Full changelog](https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/dracon-sync-v0.113.91...dracon-sync-v0.113.92)
