# Scheduled runner namespace incompatibility — 2026-10-18

## Approved IAM change executed

Operator explicitly authorized the exact fixture-prefix addition to the existing
uploader GET+PUT and consumer GET object Allow resources. Both policies were
read, compared before modification, imported through the documented OVH POST
policy API, and read back exactly. Existing games/* access, all Deny statements
and all other statements were preserved. No credential, bucket or namespace
changes. See iam-receipt.json (10:34:55/56 UTC). An initial PUT returned 405;
the API schema identified POST as the import method; the failed PUT did not
modify a policy.

## Remaining reproduced root cause

Scheduled service attempted publication at 10:37:20 UTC and refused. Both
qualification objects were still absent when independently HEAD-probed.
Status-only Node HTTP diagnostics then established node processes launched,
but no HTTP response/error events before the fixture refusal at 10:40:39 UTC.
No request headers, bodies, URLs, secrets or raw exceptions were logged.

A bounded transient user-service probe reproduced the same filesystem sandbox:

```
systemd-run --user --wait --pipe --collect --unit=bridge-authority-probe \
  -p ProtectHome=read-only -p ProtectSystem=strict \
  -p 'ReadWritePaths=/home/dracon/.dracon /home/dracon/dracon/bridge-fixture' \
  /etc/profiles/per-user/dracon/bin/node \
  /home/dracon/Dev/dracon-utilities/dracon-sync/scripts/diagnose-live-bucket-authority-2026-10-18.mjs

consumer: unsupported, reason unsafe-ancestor
binding: wrong-directory-owner
```

A separate status-only stat probe under that sandbox reported `/` and `/home`
owned by UID65534 with process euid1000. Host probes report UID0 for those
ancestors; operator-owned directories still appear UID1000 in both contexts.
The systemd.exec manual confirms user-manager filesystem sandboxing implicitly
uses a user namespace, omitting the root identity mapping. Even though the
unit's displayed PrivateUsers=no, this implicit mapping applies. A transient
PrivateUsers=identity attempt failed with 217/USER; no production mapping change.

Consumer/binding/runner/storage/restore each require root or operator ownership
throughout ancestry. The UID collapse loses the distinction between root and
foreign unmapped owners, so simply trusting 65534 is unsafe. No such exception,
filter exemption, sandbox relaxation or root daemon was introduced.

## Safe state and review

Temporary 30-bucket-diagnostic.conf NODE_OPTIONS preload was removed via
sanctioned maintenance, daemon-reload and restart. Service remains active;
freeze cleared. Installed runner/daemon bytes were not changed. Exact fixture
write-path exception and corrected UK endpoint remain configured. Manifests
were not manually populated; existing assets were not retired.

Independent fresh-context reviewer 022175de recommends an administrator-managed
system service with explicit operator User/Group, the existing filesystem and
syscall confinement, and real host-root ownership visibility. This is a service
manager migration, NOT running the daemon as root. It requires explicit
administrator approval. The alternative trusted host-metadata helper needs a
separate authenticated, descriptor-bound authority design across all five
validators; raw stat snapshots alone do not solve pathname ABA races.

Reviewer performed source review only, no execution/network/credential access;
parent performed the bounded sandbox reproductions. Full report:
namespace-security-review.md. Neither the review nor the IAM receipts establish
successful upload, independent remote bytes or fresh restoration. Tasks 2 and
3 remain open. No completion claim is made.
