## dracon-sync v0.113.17 — table: truthful REM, CHANGES column, 🔓

Three changes from one round of operator feedback on the live table.

### REM shows ACTIVE push remotes only

Excluded remotes are no longer rendered dim — the dim styling was
invisible in pastes and read as "every repo has github+gitlab+
codeberg", which is wrong (most of the fleet pushes github+gitlab
only; codeberg is gated by the public-only policy and the v0.112.28
quota posture). Now `🐙🦊` is the honest common case; exclusion
detail lives in `repos <name>` and the JSON row.

### CHANGES column split out of ACTIVITY

ACTIVITY holds only the state label (`⏳ dirty 0m` / `🟢 synced 19m` /
`⚪ idle 6h` / `⚫ cold 1d`). Everything modified/staged/untracked/
excluded (`1 mod`, `1 mod 1 excl`) renders in its own CHANGES column,
`—` when clean. The rich-tier floor stays ≤ 165 cols.

### 🔓 public marker joins 🔒

The REPO cell now shows BOTH github visibility states (cache-driven);
unknown/unprobed repos get no marker.

Upgrade: `cargo install dracon-sync --locked` or your usual path.
