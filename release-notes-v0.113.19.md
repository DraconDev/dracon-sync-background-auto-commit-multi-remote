## dracon-sync v0.113.19 — per-class change columns + SIZE fallback fix

### Changed

- **CHANGES split into four per-class columns** (operator: "the
  changes should be in their respective columns, not just dumped
  there") — 📝 modified · 📦 staged · 🆕 untracked · 🚫 excluded,
  icon headers, dim `—` when the class is clean. 5-wide columns so a
  3-digit count (junk-runner's 282 modified) fits unclipped. Table:
  16 columns / 158 cols, still inside the 165-col rich-tier floor.

### Fixed

- **SIZE column: `du -sb` fallback double-counted submodule
  gitdirs.** The `count-objects` fast path measures only the repo's
  own object store; the fallback descended into `<gitdir>/modules/`
  and would have reported a superproject's own pack PLUS every
  submodule's gitdir (each already counted in the nested repo's own
  row). The fallback now subtracts `modules/`, so both paths agree.
  Verified on dracon-platform: the 12 GiB SIZE is the parent's own
  genuine pack; the 7.7 GiB of modules correctly lives in the game
  repos' own rows — the fast-path calculation was already right.

1204 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
