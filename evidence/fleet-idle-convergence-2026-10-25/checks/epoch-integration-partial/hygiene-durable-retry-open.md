# Durable artifact-ignore retry — original gap and required boundaries

UPDATE: implemented in hygiene durable v2; coherent four-gate intermediate proof
is hygiene-durable-v2-receipt.json. Independent code review remains pending; no
final acceptance/deployment claim. See CURRENT.md for the authoritative state.
The dataflow and boundaries below are the ORIGINAL preimplementation assessment,
not proof that every interleaving/control has been independently qualified.

## Demonstrated dataflow gap

`Staging::finish` derives directory ignore rules from captured HEAD artifact
paths; `Epoch::ignore_patterns` retains those rules only in memory.
`stage_commit_and_push` commits the de-index first, then calls
`apply_deferred_ignore`. An index-lock/refusal before ignore preparation, or a
parent exit after committing but before applying the rules, can lose this intent.
On a later pulse the artifact paths are absent from HEAD, so the original rule
cannot be rediscovered by the tracked-path detector. Reporting pending in the
first call is necessary but insufficient. Existing generic restore settlement
classifies/archives untouched desired bytes; it does not replay ignore intent.

## Required durable implementation boundaries

- Persist an explicit, typed artifact-ignore intent BEFORE the index-side commit
  can become visible. Retained private staging directories/packs already survive
  installation, but their existence alone is not a recognized scheduling or
  remediation signal.
- Bind repository identity, captured artifact path/OID provenance and directory
  components. Never turn arbitrary JSON strings or a broad exclusion waiver into
  an ignore grant. Derive positive directory rules again from verified provenance
  and CURRENT effective cleanup/exclusion policy. Revoked/unknown authority is not
  permission to apply old bytes.
- Do not persist/replay stale large-file rules indiscriminately: those files
  remain available to the corrected admitted-path/size scanner on the next pulse.
  Private assets and auto-stage opt-outs must remain outside that scanner.
- Retry by rendering from a NEW verified canonical ignore observation, preserving
  concurrent user content; never copy a previous rendered whole file over it.
  Reuse the descriptor/lease/exchange machinery and original-observation token.
- Failures before a new helper journal exists must still retain the old intent.
  Retirement requires verified rules present, successful application followed by
  truthful metadata staging follow-up, or an explicitly classified supersession.
  Never clear a record merely because a helper exited or a marker was written.
- Keep metadata intent distinct from uncertain canonical recovery. A precommit
  intent with an owned staged artifact deletion must not enter a recovery gate
  that blocks the very commit needed to drain it. Foreign staging remains fully
  protected. Cheap any-concern scheduling and canonical-recovery commit refusal
  cannot be conflated into one Boolean.
- Include the new intent in scheduling, read-only reporting and idle acceptance;
  inspection failures or unsupported operations remain visible, not healthy.
- Use private nonsymlink ownership-checked paths, create-new writes, file/directory
  fsync and finite deadlines. No manual real-repository commit/push or policy
  migration is needed to implement/qualify this.

## Required disposable controls

1. Real production artifact de-index succeeds, canonical ignore write is refused;
   restart/pulse rebuilds the rule despite the path now being absent from HEAD.
2. Parent death immediately after the index-side commit cannot erase retry intent.
3. Active writer or foreign index ownership defers the metadata write, preserving
   every staged/canonical/displaced byte and allowing independent backlog work.
4. A newer canonical ignore edit survives retry; meaningful helper failure remains
   pending; current cleanup opt-out cannot be bypassed by an older record.
5. An uncommitted intent does not deadlock its own staged artifact commit.
6. Successful canonical metadata write stays pending until ordinary autonomous
   staging/publication catches up; an idempotent no-op creates no fake work.
7. Malformed/foreign/symlinked intent is refused and diagnosed without mutation.

All full final gates, independent review, loaded-runtime proof, original product
gates and every-watched clean/idle acceptance remain required after this work.
