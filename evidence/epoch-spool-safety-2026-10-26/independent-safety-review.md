## Review

- Scope: read-only inspection of current source at launch HEAD `48a2e6704105`, not a committed-range diff. Working tree was unchanged. No tests, cleanup, edits, or full suites performed.

### Correct
- `src/sync/staged_epoch.rs:240–261`: successful/empty release explicitly preserves directories containing `original-index` or `preserve-rejected`.
- `src/sync/staged_epoch.rs:1028–1046`: rejection marker is written before pruning the selected snapshot. A marker-write failure returns an error without deleting the retained directory.
- `src/sync/staged_epoch.rs:1056–1065`: an empty selected candidate cannot delete a marked original candidate.
- `src/sync/unmerged_epoch.rs:382–423`: original conflict index and independent payload pack are retained before actual-index installation; returned epoch references that same directory.
- `src/sync/staged_epoch.rs:574–585,658–688`: release follows confirmed local ref publication, including failed/timed-out follow-up. Unknown publication outcomes retain recovery. This proves local publication, not remote convergence.
- Cancellation before the release handler conservatively leaves the kept directory; neither `SpoolLease` nor the automatic reaper has a destructive drop path (`src/git/epoch_spool.rs:6–26`).
- Automatic spool reaping is a literal no-op; explicit age-only apply refuses deletion. Inventory only resolves paths, reads directory entries and metadata (`src/git/epoch_spool.rs:18–69`).
- Temporary-pack sweep has no removal operation and always returns zero reclaimed bytes/files (`src/git/mod.rs:4921–5003`). No hidden deletion found in these inventory/sweep implementations.
- Existing tests cover ordinary successful release, pre-publication deadline retention, empty-original release, rejected payload packing, marker preservation, and aged temporary-pack preservation.

### Findings
- **P2 — Misleading safety/lifecycle comments.**
  - `src/sync/staged_epoch.rs:69–76,230–251,650–652` still claim a live lease registry and age-based retry/reclamation. `SpoolLease` now only stores a path and the reaper does nothing.
  - `src/git/mod.rs:4892–4920,4981–4982` still describe deletion, cooldown, and old mtime as proof of abandonment; actual code deliberately preserves everything.
  - `src/git/mod.rs:5358–5360,5391` describe reclamation even though assertions require preservation.
  - Smallest fix: update comments/test names to the current fail-closed contract, including indefinite recovery retention.

- **P2 — Safety-boundary integration coverage remains incomplete.**
  - `src/sync/staged_epoch.rs:1164–1174` tests synthetic markers directly; the rejection test at `1296–1364` checks the pack **before publication**, not after successful release handling.
  - The post-publication deadline test (`1467–1515`) uses `Epoch::capture`, which has no retained spool; it cannot verify timeout-path spool release/preservation.
  - Smallest useful additions: real staging→rejection→successful commit with rejected-pack verification afterward; all-rejected→empty selection retention; ordinary retained-spool publication followed by post-hook timeout; actual conflict-recovery publication retaining original evidence. These test wiring across the destructive boundary rather than duplicating marker-unit assertions.

### BLOCKERS
None established within the named current-source scope.

### Merge verdict
**OK with notes** for this bounded safety correction only. This does not approve the entire fleet goal, prove bounded disk growth, or validate earlier age-only deletions. The design document correctly states those historical losses cannot now be ruled out.