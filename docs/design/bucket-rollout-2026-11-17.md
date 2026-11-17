# Bucket-backed asset rollout and commit recovery

Authorized by the operator's “ok lets do it” following the four-part storage
recommendation. Completion requires implemented and verified behavior, not
configuration files alone. No history rewrite, force-push, quota purchase or
mirror removal is authorized by this rollout.

## Required outcomes

1. Recover Deathrun's ordinary guarded commits and publishing. Correct the
   guard's distinction between repository storage and outgoing transfer size;
   unknown/new remotes must still receive a complete-history check. Verify
   actual automatic commits and remote acceptance of the preserved art work.
2. Establish bucket-backed assets for Deathrun and browser extensions, and a
   repeatable enrollment path for the asset-heavy fleet. Inventory scope and
   authority first. Keep code, exact asset references, provenance and processing
   recipes in Git. Treat original AI artwork as archival source, not disposable
   or bitwise-regenerable merely because a prompt exists.
3. Qualify immutable uploads, authenticated read-back, protected publication,
   fresh-checkout restore, existing art/build/release gates and offline shipped
   behavior before changing placement. Existing original files and historical
   versions remain recoverable. Preserve secret and manifest integrity guards.
4. Separate local commit durability, pending asset preparation and per-remote
   publication failures. Report exact blockers and preserve independent healthy
   mirror progress. Remote quota failures must not masquerade as missing repos
   or ownership/spool failures.
5. Produce a measured historical-storage inventory and a concrete archival or
   migration proposal. Execution of history-changing migration remains a
   separate operator decision. Bucket enrollment alone cannot reclaim reachable
   historical blobs from GitLab.

## Initial evidence (2026-11-17 10:46 UTC)

Deathrun still fails its pre-commit guard. Source inspection shows
`measurePushableBranch` enumerates `rev-list --objects --all` and applies the
same complete reachable-history size to every remote, regardless of the
tracking tips it reports. Restoring tracking-tip metadata alone cannot repair
this conflation. Keep a repository-growth measurement, but apply per-push
ceilings to the actual ref-scoped outgoing objects with conservative treatment
of unknown/new remotes and staged bytes.

Neither target repository has a `bucket-runner.json` or legacy asset workflow
configuration. The existing runner needs qualification for these assets;
successful synthetic enrollment or a configured credential profile is not
evidence of live archival-source coverage.

## Completion evidence to retain

- Regression and full applicable gate logs, including negative guard controls.
- Source and deployed artifact identities where executable code changes.
- Asset inventory, immutable object and manifest digests, upload/read-back and
  independent restore receipts; credentials and source secrets excluded.
- Original art preservation and the existing custody/build/release results.
- Actual commit and both-mirror checkpoint witnesses, with unresolved provider
  quotas reported accurately rather than marked successful.
- Historical inventory and separately reviewable migration plan.
