# Standard LFS adoption: implementation/qualification report

Date: 2026-10-11. Status: implementation deferred; the live pilot and security-authorization gate require external actions outside this session.

## Decisions taken

- Target protocol: standard Git LFS v1 with SHA-256 identity. The existing
  strict pointer encoder at `dracon-sync/src/storage_core/reference.rs` is
  preserved as the pointer contract for any future Sync-managed route, but
  is not connected to the stock `git-lfs` client or a maintained server.
- Fast path: ordinary small repositories (illustrative budget 1 GiB local
  Git history, 20 MiB individual files) remain in Git with no bucket
  dependency. No automatic asset enrollment.
- Pilot game: `polis` (web/games/wip/polis) was inspected as a candidate
  forward-only pilot because its inventory (`web/docs/bucket-migration-inventory.json`)
  has explicit classification and the repo is private on GitHub and GitLab.
  Pilot activation requires an additional operator decision.
- Backend: configurable. OVH S3 endpoint (`s3.uk.io.cloud.ovh.net`) is the
  documented default; private-source traffic must use a separate, private
  bucket or namespace from the public asset bucket `dracon-master`.

## What is not yet implemented

- No maintained LFS server is selected, installed, or tested. `git-lfs` is
  not present on this host; `git-lfs-server`, `lfs-test-server`, `gitea`,
  `forgejo`, `gitlab-workhorse`, `garage`, `minio`, `rclone`, `restic`,
  and `kopia` were not found on `PATH`. Each absence is a non-exhaustive
  install search; no remote package audit was performed.
- No private OVH bucket or IAM principal exists for source assets. The
  public asset bucket `dracon-master` is unsuitable for private source
  LFS traffic.
- The Warden global pre-push hook (`~/.config/git/hooks/pre-push`) chains
  foreign and repository-local hooks before its own checks. A stock LFS
  pre-push would therefore upload bytes before any refusal. This is a
  blocker for automatic LFS routing and is not yet fixed.
- No daemon-side LFS path exists. `dracon-sync/src/storage_core` holds
  only the custom strict pointer encoder, the local backend, and an
  S3-compatible adapter used by the custom pipeline, none of which speak
  the LFS API.
- No recovery story is implemented. The previous design plan
  (`lfs-adoption-plan-2026-10-11.md`) requires an independent backup of
  LFS objects and required server metadata, which is not present.

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
