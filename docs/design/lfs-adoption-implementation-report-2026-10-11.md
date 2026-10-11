# Standard LFS adoption: implementation/qualification report

Date: 2026-10-11. Status: isolated policy primitives implemented; production routing and live qualification remain unfinished.

## Decisions taken

- Target protocol: standard Git LFS v1 with SHA-256 identity. The existing
  strict pointer encoder at `dracon-sync/src/storage_core/reference.rs` is
  preserved as the pointer contract for any future Sync-managed route, but
  is not connected to the stock `git-lfs` client or a maintained server.
- Fast path: ordinary small repositories (illustrative budget 1 GiB local
  Git history, 20 MiB individual files) remain in Git with no bucket
  dependency. No automatic asset enrollment.
- Pilot candidate: `polis` (`web/games/wip/polis` in the sibling
  `dracon-platform` checkout), not a qualified or enrolled pilot. The prior
  inventory reference is outside this report's audit boundary; the parent
  utilities `web` symlink cannot serve as durable inventory evidence. Neither
  asset approval nor present forge privacy is established by this report.
- Backend: configurable. OVH S3 endpoint (`s3.uk.io.cloud.ovh.net`) is the
  documented default; private-source traffic must use a separate, private
  bucket or namespace from the public asset bucket `dracon-master`.

## What is not yet implemented

- No maintained LFS server is selected, installed, or tested. `git-lfs` is
  not present on this host; `git-lfs-server`, `lfs-test-server`, `gitea`,
  `forgejo`, `gitlab-workhorse`, `garage`, `minio`, `rclone`, `restic`,
  and `kopia` were not found on `PATH`. Each absence is a non-exhaustive
  install search; no remote package audit was performed.
- No private source OVH binding or scoped IAM principal has been qualified
  by this work. This is not proof that none exists elsewhere. The public
  asset bucket `dracon-master` is unsuitable for private source LFS traffic.
- Deployment host: none qualified or identified for LFS deployment. An
  S3 endpoint is storage, not an authorized LFS API deployment host.
- The Warden global pre-push hook (`~/.config/git/hooks/pre-push`) chains
  foreign and repository-local hooks before its own checks. A stock LFS
  pre-push would therefore upload bytes before any refusal. This is a
  blocker for automatic LFS routing and is not yet fixed.
- No daemon-side standard LFS integration exists. `src/storage_core`
  also contains capture, journals, hydration, staging, security, transfer
  and recovery modules for the existing custom pipeline. These are retained;
  their existence does not prove stock-client or LFS API interoperability.
- No recovery story is implemented. The previous design plan
  (`lfs-adoption-plan-2026-10-11.md`) requires an independent backup of
  LFS objects and required server metadata, which is not present.

## Isolated local policy milestone

`src/lfs_policy.rs` exports a non-mutating evaluator with explicit tuning and
per-repository override resolution. It checks pressure, file size and churn,
sticky representation, classification, effective-filter conflicts, opt-out and
approved-binding evidence. Invalid thresholds fail closed. No deployment
threshold defaults are established by the illustrative test settings.

Bounded checks run successfully:

```sh
timeout 180 cargo test -p dracon-sync --lib --locked lfs_policy::tests
# 11 passed, 0 failed
timeout 180 cargo clippy -p dracon-sync --lib --locked -- -D warnings
# passed (library only)
```

A read-only reviewer found no defects in this isolated milestone and requested
sticky-state cross-product coverage before integration. The added test checks
160 effective-LFS combinations across ledger presence, classification, binding,
opt-out and missing/valid/invalid/disabled tuning; each retains LFS or holds.
The reviewer did not independently rerun the tests or approve production use.

These tests use typed synthetic evidence, not real file classification or Git
repositories. The 200 MiB/small-image case models repository history and a 10 MiB
candidate. SQL source is represented by `SourceText`; no SQL-content classifier
is implemented. The >100 MiB case is a selection decision, NOT a staging-size
exception. A positive decision does not authorize uploads. There is no TOML
schema integration, attribute enrollment, dispatcher, consumer bootstrap,
transfer scheduling, stock-client compatibility test or live verification yet.
The full verification contract and tasks 2–5 remain unfinished.

## Concrete unblockers

1. Install a maintained standard LFS server shortlist candidate on an
   authorized host. Recommended: deploy a single candidate under an
   approved service unit and review its security/maintenance posture
   before broader rollout.
2. Create a private OVH bucket and a dedicated IAM principal with
   read/write scoped to the pilot repository. Do not reuse the public
   asset credentials.
3. Define and install an independent backup target for LFS objects and
   the server's own metadata. Verify restore by SHA-256 OID on a fresh
   host.
4. Authorize a hook-order change: payload authorization must run before
   any foreign or local pre-push hook. This requires a Warden design
   change (see `dracon-warden/src/main.rs:5463-5897` pre-push template).
5. Install and configure the `git-lfs` client on the producer host
   (this one) and on a separate, fresh consumer host. Both must reach
   the LFS server using the dedicated credentials.
6. Select a representative private game and approve a single new
   non-secret binary fixture under a deliberate path
   (e.g. `lfs-pilot/fixture.bin` ≤ 1 MiB in `polis`). No existing
   tracked file is converted and `git lfs migrate` is not used.

## Boundaries preserved

- No public asset contract was changed.
- No live filters, hooks, or remote configuration were modified.
- No production credentials were opened, used, or stored in this tree.
- No history rewriting, retention deletion, or fleet-wide enrollment
  was performed. The existing custom storage module is untouched.

## Suggested next step

After the unblockers above, the next goal can install and qualify the
shortlisted server, implement the daemon-side LFS path, fix the Warden
hook order, and run a forward-only pilot on `polis` with the chosen
fixture. The verification contract and bounded test fixtures defined in
the adoption plan remain the acceptance gates.
