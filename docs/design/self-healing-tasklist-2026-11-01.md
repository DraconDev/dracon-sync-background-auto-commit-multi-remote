# Self-healing pipeline — task list (2026-11-01)

Goal of this list: publication keeps working **without a human digging into
`~/.local/state/dracon/sync/gates/**`**. Every incident this session had the same
shape: the pipeline knew something failed but never said *what*, and never
recovered on its own.

Ordered by leverage. Each task states the evidence that motivates it and the
acceptance that closes it.

---

## Already done (context, not open work)

| fix | evidence |
|---|---|
| Retry budget is **per-revision** (`failed_tip`) | browser stalled forever on `2cf8c70e`, 36 commits behind HEAD; now auto-resumes |
| Gate **checkpoints** reuse passed leaf workspaces | one attempt reused 23 gates instead of redoing 44 workspaces |
| Stale **ref/HEAD lock** janitor | `refs/heads/main.lock` wedged dracon-platform 6.3 h |
| **tmp_pack** reaper with `fuser` proof | 12.60 GiB reclaimed; daemon reaped 2 packs live |
| Report truth (`ok` was a residual) | 14 dirty rows previously reported as 36 clean |
| CLI layout: no wrapped cells | `📤 PUBLISHING` wrapped → looked like an extra row |

---

## T1 — Surface the gate-failure cause (P0, no dependency)

**Evidence.** Today the operator sees
`Stuck Ahead (Unpushed): commits not reaching origin for >10 min — push may be failing`
and the hint says `run dracon-sync repair concerns --apply (push or rewrite)`,
while the real cause was `bun install` → `Workspace not found` at `2cf8c70e`,
then `virtual-pet-new-tab TEST failed`. The cause exists only inside a retained
attempt directory.

- [x] **T1.1** Stuck-ledger `last_error` carries the failing **workspace, gate
      and log directory** via `hook_failure_summary()` (pure, bounded to 200
      chars, front-loaded). Done — deployed `679347be1104`.
- [x] **T1.2** The alert/HINT inherit it: `push_stuck_hint` reads the ledger
      error, and its budget was raised 60 → 160 chars so the cause survives.
      Done.
- [x] **T1.3** Durable artifact at `<state>/dracon/gate-failures/<repo>.json`
      (atomic temp+rename, no credentials, `detail` names the gate + logs).
      Done.
- [x] **T1.4** HINT no longer says "push or rewrite" for a gate refusal once the
      budget is exhausted — it names the gate. Done.
- [x] **T1.5** Two regressions + negative controls (neutralising both made 3
      assertions fail); four gates 3018 passed / 0 failed; deployed with
      loaded == installed == built. Done.
- [ ] **T1.6** *Residual:* live artifact population is unverified — no hook
      refusal has happened since deployment. Confirm the first one writes
      `gate-failures/<repo>.json` and that the ledger error names the gate.

**Acceptance.** After a deliberately broken test in a fixture workspace, `repos`
and the alert both name the workspace, the gate and the log path — verified with
no manual log reading.

## T2 — Make the largest bucket manifest declarable (P1)

**Evidence.** `books.json` declares 952 assets / **571.9 MB** — the largest
offload — but cannot be wired into `bucket-migration-inventory.json`:
`bucket-strategy check` rejects it because its `objectKey` uses **literal
digests** (952/952) while the schema requires `…/{sha256}.{ext}`. The runtime
ignores the field (`namespace/sha256.ext`), which is why it serves correctly
while being unvalidatable. `bucket-sizes` therefore prints
`books | not declared` and cannot see 572 MB of 578 MB.

- [ ] **T2.1** Rewrite the 952 `objectKey` values to `books/{sha256}.{ext}`,
      proving the rewrite is a no-op against the real key derivation.
- [ ] **T2.2** Wire the manifest into the `books` inventory record and confirm
      `bucket-strategy check` and `bucket-sizes` both accept it (expect
      571.9 MB reported).
- [ ] **T2.3** Operator decision: leave `classification: legacy`, or promote to
      `declared` (a migration-state claim, not a mechanical change).

**Acceptance.** `bucket-sizes` reports ~572 MB for books; `check` stays green.

## T3 — Remove the decorative `objectKey` divergence (P2)

**Evidence.** Three key shapes coexist: canonical template (`games.json`),
slug-prefixed template (`games-private.json` → `games/covers/<slug>/{sha256}`,
which the runtime ignores), and literal digests (`books.json`). `bucket-sizes`
reads `asset.objectKey`; `bucket-asset-transfer` derives its own. That
divergence made my first verification report **1/12** assets present.

- [ ] **T3.1** Decide: delete the field, or make every consumer use one
      derivation.
- [ ] **T3.2** Add a schema check that rejects a manifest whose `objectKey`
      disagrees with `namespace/sha256.ext`.

**Acceptance.** A manifest with a wrong `objectKey` fails validation instead of
silently computing a key that does not exist.

## T4 — Bucket coverage for the oversized repos (P1, needs classification)

**Evidence.** `ai-auto-writer` is **7.25 GiB** with no bucket manifest and is not
in the inventory (which covers only `dracon-platform`, 32 records). On-disk
history by class: `.jpg` 2.24 GiB (31%), `.png` 2.20 GiB (30.5%), `.epub`
1.29 GiB (17.9%), `.md` 1.08 GiB. `.log` is 4.59 GiB raw but **0.01 GiB** on
disk — a red herring that cost me a wasted slimming run.

- [ ] **T4.1** Operator classification: are `.epub` (built from `.md`) and
      `cover.png`/`.jpg` regenerable artifacts or shipped product?
- [ ] **T4.2** If regenerable: manifest + move to the bucket, gitignore.
- [ ] **T4.3** Sanctioned slimming with a verified bundle; tip tree must stay
      byte-identical where the class is history-only.
- [ ] **T4.4** Verify: guard passes, all remotes agree, reachable pack measured
      before/after.

**Acceptance.** Reachable pack below the repo's budget with no shipped content
lost, or a recorded decision that the size is accepted.

## T5 — Restore `freeport`'s git identity (P0, operator-only)

**Evidence.** `user.name`/`user.email` are literally `--global` (a misordered
`git config user.name --global …`). 60 commits authored `--global <--global>`
since 15:04, already published; daemon warns `path_owned:untrusted_email`.
No other repo is affected.

- [ ] **T5.1** Operator runs the two `git config` commands (agents may not).
- [ ] **T5.2** Verify the next commit's author is `DraconDev <dracsharp@gmail.com>`.
- [ ] **T5.3** Optional: mailmap the 60 published commits so history displays
      correctly (no rewrite).

## T6 — Spool reclamation (blocked, do not start)

**Evidence.** 213 dirs / 0.77 GiB fleet-wide and slowly growing.
`LOSSLESS_RECLAIM_QUALIFIED = false`; `src/git/epoch_spool_*.rs` is under active
concurrent redesign with its own revision-pinned test helper.

- [ ] **T6.1** Wait for that redesign to land and qualify.
- [ ] **T6.2** Then reclaim with content proof (not age), preserving
      spool-only objects as was done for ai-auto-writer's 9 objects.

## T7 — Investigate a new signal (P2)

**Evidence.** Alert observed:
`capture-anime-girls — Sync Failures (backing off): 5 consecutive sync failures;
probing every 15 min`. Not yet diagnosed.

- [ ] **T7.1** Identify the failing phase from the journal and the retained
      epoch/staging artifacts; decide whether it shares a root cause with T1.

---

## Standing rules for this list

- Every fix ships with a regression **and** a negative control that fails when
  reverted; no gate is narrowed or skipped.
- Deployment requires loaded == installed == built and a green four-gate run; a
  load-induced timing flake is retried, never excused by weakening a test.
- Agents do not run `git config user.*`, rewrite history, force-push, or stop
  producers (T5 is operator-only).
- Any claim about bucket contents must be verified by fetching the object, not
  by reading a manifest.
