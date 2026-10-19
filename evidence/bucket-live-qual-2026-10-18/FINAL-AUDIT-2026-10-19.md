# Final audit — deployed OVH bucket path (2026-10-19)

Scope: the deployed live bucket path (daemon bridge → runner bundle →
dracon-lfs) and every preceding qualification item. All checks below were
re-run fresh on 2026-10-19 unless marked historical. No crate release,
fleet-wide asset migration, credential expansion, history rewrite, guard
bypass or unrelated cleanup was performed.

## Item → evidence map

| # | Preceding item | Concrete evidence (fresh check) |
|---|---|---|
| P1 | Qualified production artifacts + docs; scoped credential-file access | Runner bundle source == installed == profile-pinned SHA256 352e4de0…3020; daemon source version 0.113.95 == installed 0.113.95; credential files 600/dracon (stat 2026-10-19) |
| P2 | Sanctioned deployment, atomic install, rollback, trusted runner identity | Operator-UID system service active from /run (v8, frozen plan a73bad17…); rollback `handover-backup` retained in v8 dir; 17/17 migration tests re-run OK |
| P3 | Live OVH upload + fresh restoration qualification | QUALIFICATION-RESULT-2026-10-19.md; consumer GET + fresh restore re-run exit 0 for fixtures 20261018 and 20261019 on 2026-10-19T09:26Z (receipts refreshed) |
| P4 | Autonomous live retry under local transport failure | RETRY-QUALIFICATION-RESULT-2026-10-19.md (audit-approved); profile restored byte-exact 5c4b1673…; retry consumer/restore re-run exit 0 2026-10-19T09:26Z |
| P5 | This final audit | This report + gate outputs below |

## Routing design reconciliation

- Design (dracon-platform/web/docs/bucket-strategy.md): one shared
  bucket, namespaced keys, cache-first dev resolution, public URLs only
  for regenerable+public assets, offline deterministic planner,
  fail-closed high-water guard; dracon-master promotion is explicitly
  offline/forward-only and "does not authorize live bucket mutations".
- Deployed live path (what was qualified): private bucket `dracon-lfs`,
  namespace `dracon-platform/f5829561-d50d-4644-8e6e-ad7e293fa1db`,
  content-addressed keys `<namespace>/<sha256>.png`, scoped uploader
  (GET+PUT) / consumer (GET) credential files, manifest visibility
  `private` → no public URLs emitted; restoration is via the scoped
  consumer transport. Consistent: the live path does not use the
  dracon-master public promotion route, and the planner remains offline.
- IAM scope unchanged since the operator-approved receipt
  (iam-receipt.json): fixture prefix added 2026-10-18 with exact
  readback; games/* and all Deny statements preserved.

## Source / installed identity

- dracon-sync: installed /home/dracon/.local/bin/dracon-sync
  SHA256 2b74bb7c…fdba, version 0.113.95 == dracon-sync/Cargo.toml.
  No bucket-path source changes in the nested repo since 2026-10-18.
- Runner: dracon-sync/scripts/bucket-runner/runner.bundle.mjs ==
  ~/.dracon/bin/runner.bundle.mjs == profile-pinned 352e4de0…3020.
- Service: dracon-sync-operator.service (system, /run, Type=exec,
  User=dracon, MainPID 2018106 all-UID 1000, exe hash matches plan);
  user unit masked; operator watchdog timer active.

## Quality gates (fresh, 2026-10-19)

- `cargo test -p dracon-sync --locked`: 1517 passed, 0 failed
  (144 + 1349 + 10 + 14; bucket-filtered subset 42 passed).
- `cargo clippy -p dracon-sync --locked`: clean (exit 0).
- `node --test dracon-sync/scripts/bucket-runner/*.test.mjs`: 157/157.
- Migration suite: 17/17 OK. Prepared NixOS module:
  `nix-instantiate --parse` OK.

## Independently verified remote bytes + fresh restoration (fresh)

2026-10-19T09:26Z, all exit 0, byte-identical, digests match sources:
fixtures 20261018 (215dc3a0…), 20261019 (26da6100…), retry (e842d3f0…).
Restores: empty root, exactly 1 download each, restored byte-identical,
retry already-present, manifests unchanged. Receipts:
independent-consumer*.json, retry-consumer-20261019.json,
fresh-restoration*.json, retry-restoration-20261019.json.

## Retry behavior / unrelated sync progress

P4 stands (approved): failed attempt committed nothing unverified
(manifest unchanged, remote 404); autonomous retry uploaded at
09:11:12Z with exactly one enrollment. Today: `dracon-sync health`
healthy; 11 "synced" events in a 7-minute sample; bridge-fixture CLEAN
apart from the known untracked 20261018 source dir (tracked PNG +
manifest committed); unrelated repos commit/sync continuously.

## Findings

- **F1 — operator action (prepared, not applied): reboot persistence.**
  System units live in /run (ephemeral); user mask persists. After a
  reboot no scheduler starts until the operator either applies the
  prepared declarative module `dracon-sync/scripts/dracon-sync-operator.nix`
  (syntax-checked; exact activation/rollback steps in its header) or
  restores user scheduling (which reintroduces the sandbox ownership
  refusal). Impact is sync downtime only — no bucket data risk; health
  checks make it immediately visible. This is an operator-owned
  deployment decision (they executed the runtime migration knowingly),
  not a bucket-path correctness/safety defect.
- **F2 — known limitation, fail-closed (out of qualified scope):**
  dracon-platform root still refuses `accounting-unknown-filter-bound`
  (worktree filter accounting refuses to bound warden-filtered files).
  Platform has no bucket-assets manifest and its design posture is
  offline/forward-only, so nothing is published unverified; the cost is
  platform-root auto-commit blockage, owned by a separate daemon
  accounting design decision. No guard was bypassed to change this.
- **F3 — historical caveat, mitigated:** the first fixture's upload
  (2026-10-18) is not attributable to the scheduled daemon
  (upload-provenance-finding-2026-10-19.md); the second fixture carries
  the decisive autonomous proof, and both re-verified today.
- **Operational note:** two foreign-session daemon freezes were found
  forgotten (owning processes gone) and resumed via the sanctioned
  `dracon-sync resume`; the 30-minute freeze watchdog backstop exists.

## Verdict

The deployed OVH bucket path is qualified end-to-end: routing matches
design intent, installed artifacts match source and pinned identities,
all quality gates pass, remote bytes and fresh restoration are
independently verified for every qualified fixture, retry behavior is
proven, and unrelated sync progresses. No unresolved in-scope
correctness or safety findings remain; F1/F2 are operator/design
decisions documented above with prepared remediation for F1.
