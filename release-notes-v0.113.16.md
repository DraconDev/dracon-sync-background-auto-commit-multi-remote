## dracon-sync v0.113.16 — REM truth fix + 🔒 privacy marker

Driven by three spots from the operator reading the live table.

### Fixed — REM column lied about codeberg

The report's push-remote computation applied only the
codeberg-public-only visibility gate and missed the daemon's
v0.112.28 quota-posture rule (codeberg skipped at push time when the
repo has no codeberg tracking ref AND auto-create is off). convos,
dracon-libs, practice-form and DraconDev showed a BRIGHT 🗻 while the
daemon deliberately skipped codeberg — a silent push-gap lie. The
report now runs the daemon's FULL filter (`report_effective_remotes`)
and `codeberg_skip_reason` gains a `"quota"` variant so quota skips
are distinguishable from visibility skips.

### Added — 🔒 private-repo marker

REPO cell renders `name 🔒` when the github visibility cache says
private; unknown/unprobed repos get no marker. Legend explains it.

### Ops (same batch, config-side)

- hellhunter's 🚫 unowned was the ownership guard working correctly:
  the new phase-e agent loop committed with an unwhitelisted
  identity. Whitelisted `phase-e-agent <phase-e@local>` per the
  AGENTS.md new-loop procedure.
- junk-runner's repo-local identity was the placeholder
  `dracon@example.com` (masked from the guard by its `owned = true`
  override) — fixed to the canonical `dracsharp@gmail.com`.

Upgrade: `cargo install dracon-sync --locked` or your usual path.
