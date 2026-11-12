# Spool reclaim throughput — the leak that a correct proof could not stop (2026-11-12)

Related: `epoch-spool-disk-leak-2026-10-26.md`,
`epoch-spool-durable-recovery-2026-10-31.md`,
`audit-stale-binary-and-spool-bloat-2026-11-07.md`.

## Symptom

`dracon-sync health` was green while aged epoch spools accumulated again:

| Repo | Spools | Size | Oldest |
|---|---|---|---|
| dracon-platform | 3,668 | 6.5 GiB | 2026-11-02 |
| browser-extensions-shared | 1,167 | 2.7 GiB | 2026-11-07 |
| ~/.dracon | 7,474 | 1.3 GiB | 2026-11-07 |

dracon-platform alone created ~200 spools/hour (one per epoch). Over 24h the
redundant reclaimer removed 534 spools fleet-wide.

## Why spools are not released at publication

`LOSSLESS_RECLAIM_QUALIFIED = false` (`src/git/epoch_spool.rs`) is a deliberate
safety hold: the qualified reclaim transaction is interlocked off, so every
epoch's spool is retained and logs `durable recovery qualification incomplete`.
The ONLY route that removes spools is the redundancy-only reclaimer
(`src/git/epoch_spool_redundant.rs`), which deletes a spool only when every
byte it holds is reachable from a published remote-tracking ref. That proof is
sound and was not changed. Its *scheduling* was the defect.

## Root causes

1. **Throughput ceiling below the creation rate.** One pass per repo per hour
   (`REDUNDANT_RECLAIM_COOLDOWN_SECS = 3600`), at most 64 spools per pass
   (`MAX_SPOOLS_PER_PASS`): ≤ 1,536 spools/repo/day vs ~4,800/day created. The
   leak was guaranteed even if every spool was provable.
2. **Head-of-line starvation.** `spool_candidates` sorts oldest-first. The
   oldest spools are the permanently unprovable ones (legacy
   `dracon-epoch-message-*` without a producer fence; spools with unknown
   payload). Journal evidence: 442 distinct spools refused in 24h, each ~15
   times; the other ~11.8k were never examined.
3. **The daemon minted unprovable spools.** The epoch path's
   `git rm --cached --ignore-unmatch` (`src/sync.rs`, "epoch missing") ran
   without `-q`; its `rm '<path>'` stdout is captured into a `producer-io-*`
   endpoint and fails `producer_names` ("not a full-width hash"). Survey:
   225 spools. (The remaining few non-name endpoints were `fatal:` stderr of
   genuinely failed epochs, correctly preserved.)

Refusal reasons over 24h: legacy ownership uncertain 3,165 · producer endpoint
not a name 1,969 · no settled marker 729 · index not in a published tree 468 ·
unreachable object 133.

## Fix (systemic, not a parameter bump)

The cooldown's rationale was "a pass that just ran cannot learn anything new
about the same spools". That is now enforced *directly* instead of by spacing:

- **Refusal memo** (process-local, keyed by spool path). A spool is skipped
  while its last refusal is still the answer:
  - `NotYetPublished` refusals (unreachable object, index entry absent from
    published history, producer name neither published nor an owned pack) are
    retried as soon as the published ref tips move (fingerprint = every
    published ref + tip, three cheap plumbing calls);
  - every other refusal is spool-intrinsic and retried after 24h;
  - any change to the spool directory's mtime invalidates the memo.
  The memo can only make the reclaimer **skip** a spool — never remove one.
  The proof is unchanged; a restart re-proves each refusal once.
- **Time-bounded passes.** Cooldown 10 min; each pass's spool loop stops
  between spools after 120 s (hard cap 2,048/pass as a pathological backstop).
  The reachability walk the old cooldown protected now costs ~2.2 s on
  dracon-platform (402k objects, load average 37) since the single-walk change.
  The pass runs inline on the repo's sync worker, so the budget bounds how long
  housekeeping can delay that repo's next commit.
- **Producer hygiene.** `-q` on the epoch `git rm`, matching the hygiene
  de-index. Deleting endpoint files after a successful producer exit was
  considered and rejected: endpoints must remain until settlement (a detached
  Git descendant may still hold them; unlinking would hide a live writer from
  the `/proc` fd liveness check).
- **Observability.** One `🧹 … spool reclaim pass:` summary per pass; each
  refusal logged once per state instead of every pass; `health` gains a
  `Spools` row (unhealthy above 5 GiB of aged spools), with the inventory run
  concurrently across repos.

## Regression tests (`src/git/epoch_spool_redundant_tests.rs`)

- `an_unprovable_oldest_spool_does_not_starve_the_backlog` — pass cap 1; the
  legacy oldest spool consumes pass 1, pass 2 must reach the redundant one.
- `an_intrinsic_refusal_is_not_retried_when_published_state_moves`
- `a_not_yet_published_refusal_is_retried_once_it_is_published`
- `a_changed_spool_invalidates_its_memo`

Non-vacuity: forcing `refusal_still_holds` to `false` fails the three memo
tests.

## Residual / follow-ups

- The legacy `dracon-epoch-message-*` population (~160 in dracon-platform,
  `COMMIT_EDITMSG` only, tiny) can never be proven by this route. It no longer
  blocks anything; removing it is an operator decision.
- The structural fix is re-qualifying the lossless transaction so a published
  epoch releases its own spool inline; the reclaimer would then be a backstop.
