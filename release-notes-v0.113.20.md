## dracon-sync v0.113.20 — superproject SIZE shows own + submodule footprint

### Added

- **SIZE column: `own+mods` for superprojects** (operator: "we made
  them submods so we don't end up with one huge repo, so it would be
  useful to know both sizes — partly to see if it would get stuck
  when pushed"). dracon-platform now renders `12G+7.3G`: own pack +
  combined submodule gitdirs under `.git/modules/`. Plain repos keep
  the adaptive form (`713 MiB`). Color still follows the own pack —
  that's what actually pushes per-push; the suffix is your
  wholesale-push gauge (a fresh first push to a new remote ships the
  whole pack).
- New `measure_modules_size_bytes` probe — one extra `du`, only for
  repos that actually have a `modules/` dir — cached alongside the
  own-size probe (new `git_modules_bytes` in the size cache;
  serde-default 0 for old cache files, self-heals at the next
  recompute).
- SIZE column widened 10 → 11 for MiB-scale combos (`446M+713M`);
  table total 159 cols, still inside the 165-col rich-tier floor.

Ground truth: dracon-platform's own pack is genuinely ~12 GiB (345k
objects, zero garbage); the 7.3 GiB of submodule gitdirs is also
reported per-game in the nested repos' own rows.

1209 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
