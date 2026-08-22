# dracon-sync v0.113.53

Released 2026-08-22. Three user-visible changes: a persistent
watched-repo-vanished concern (the last open gap from the 2026-08-21
checkout-disappearance incident), a cold-render parallelism fix
(36–50s → ~14s), and an end to false 🩹 BROKEN flags from probe timeouts.

## Added

- **Watched-repo-vanished ledger + CONCERN** (`src/vanished.rs`): the
  daemon now persists which repo paths it has synced; when one drops
  out of discovery it is logged once per episode and surfaced as a
  persistent `❌ CONCERN` by `repair concerns` (dry-run and apply)
  until the path exists again. Entries auto-expire after 90 days.
  Closes gap G2 of
  `docs/design/utilities-checkout-disappearance-2026-08-21.md`.

## Fixed

- **Cold `repos` render serialization**: the per-repo size/history
  probes are blocking subprocess calls with no await point between
  them, so `buffer_unordered(16)` never actually overlapped them —
  cold renders took 36–50s wall (sequential sum measured 32s). The
  compute now runs in `compute_cold_size_entry()` on the
  `spawn_blocking` pool; measured 42s → 13.7s cold, warm unchanged.
- **Probe-timeout false BROKEN**: ai-auto-writer was flagged
  PUSH 🩹 BROKEN / CONCERN when its ~99k-object history probe blew the
  hard 4s deadline under full-fleet concurrent probing. The bound is
  now 10s with one retry before reporting failure; genuinely broken
  HEADs still report failed after retries.
- **scp-style remote parsing** (audit M1, carried from Unreleased):
  bracketed IPv6 hosts and non-`git@` usernames in scp-form remotes no
  longer break mirror-dedup and pack-guard classification.

## Verification

- `cargo test -p dracon-sync --locked`: 996 passed / 0 failed
  (includes new vanished-ledger, cold-entry, and retry-regression tests)
- `cargo clippy -p dracon-sync --locked --all-targets -- -D warnings`: clean
- Live: cold render 42.4s → 13.7s; false-BROKEN cleared;
  simulated vanished path surfaced via `repair concerns` dry-run
