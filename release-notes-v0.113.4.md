# dracon-sync v0.113.4 — 2026-07-26

Full-audit remediation **batch 4** of `AUDIT_FULL_2026-07-26.md`.
All 13 fleet-wide HIGH findings from that audit are now closed across
this release and v0.113.2, v0.113.3, and v0.113.1; this entry covers
the two HIGHs fixed in v0.113.4 specifically.

## Highlights

- **Visibility cache-poison on transient GitHub failures (SYNC-H4)**
  fixed. `sync_mirror_visibility` now uses the `_opt` variant and
  skips BOTH the mirror flips AND the cache write when GitHub
  visibility is unknown, instead of poisoning the 24h cache with a
  safe-default `true` (which had been flipping public mirrors to
  private on network blips and gating the `codeberg_public_only`
  push path off).
- **`standard_files` source path traversal (SYNC-H5)** closed both
  at `validate_config` AND at the point of use in
  `ensure_standard_files`. New shared
  `is_safe_standard_file_path` rejects raw-absolute paths and `..`
  components (tilde `~/...` still allowed). The daemon's execution
  path never called `validate_config`, so the prior validation was
  effectively advisory; config typos or policy-file writes are no
  longer a read-anywhere → publish-everywhere primitive under the
  daemon's UID.

## Test / gate posture

- `cargo test --workspace --locked` — green.
- `cargo clippy --workspace --locked -- -D warnings` — green.
- Binary deployed to `~/.local/bin/dracon-sync`; daemon restarted.

## What comes next

- Prior batches shipped: v0.113.2 (H1/H2/H3/H7/H8), v0.113.3 (H6).
- Prior session: v0.113.1 (FilterOnly + stale-upstream).
- MEDIUM / LOW / meta items remain queued as backlog in the operator's
  `/list` (the audit's full enumeration is in
  `AUDIT_FULL_2026-07-26.md`).
- The warden 10 MiB fail-closed guardrail stays intact fleet-wide;
  junk-runner opted its `.pi-glla/**` orchestrator state out via a
  surgical `.gitattributes` bypass (audit INFRA-1, option 1).
