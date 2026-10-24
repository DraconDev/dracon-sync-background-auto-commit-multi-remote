# Autonomous fleet convergence — current diagnostic report

## Result status

Implementation and final-binary qualification are established; fleet completion
is **not yet claimed**. The current ledger proves autonomous eligible commits and
advertised descendant refs for nine of the ten initial repositories on loaded
artifact `59ef460ef335d51aafd09f01d7537589a70712254feb2557751c440f62146cfc`,
PID3046262. Browser extensions is still explicitly nonhealthy/unproven while its
normal whole pre-push gate runs. No external blocker has been accepted.

Authoritative current evidence is `checks/live-convergence-ledger.json` and its
referenced before/after snapshots and journal. The earlier8be epoch is retained
under `checks/epoch-8be-before-v6/`; it must not be mistaken for current-runtime
proof. Historical investigation/deployment updates remain linked from
`diagnosis.md`, `qualification-checkpoint.md` and `execution-update-2026-10-24.md`.

## Initial inventory and disposition

The fresh execution baseline is `baseline.json`; complete fleet state is retained
in `baseline-all-repos.json`. Activity and a successful earlier push were not
accepted as final-runtime convergence.

| Initial repository | Observed baseline | Demonstrated repair/evidence scope |
|---|---|---|
| ai-auto-writer | working;1modified/8untracked; no upstream lead | Distinguish active work from a hard blocker; require new loaded-runtime commit/ref evidence. |
| ai-auto-video | queued;1untracked | Same evidence requirement; no invented product defect. |
| pi-goal-list-loop-audit | converged at that instant | Earlier activity is not proof on a replacement daemon; new witness required. |
| dracon-strategy | stalled;75ahead | Authorized OpenAI-hook boundary precision repair, full text/binary credential regressions, qualified Warden and targeted retry. |
| .dracon | queued;1ahead | Report missing scheduler evidence truthfully rather than infer health; verify advertised destinations/aliases. |
| browser-extensions-shared | publishing2301seconds;314ahead | Missing service tools, mutable-source/dependency/lifecycle integration defects, demonstrated product failures and later native task-budget exhaustion. Whole final hook/publication remains pending. |
| endless-td | converged at that instant | Fresh final-runtime commit/ref evidence, not a cosmetic state change. |
| dracon-platform | blocked;31ahead;20modified/3untracked | Bounded private-storage workflow correction, source revalidation, original storage guards, later documented legacy sample retirement. |
| polis | converged at that instant | Correct orphan classification without terminating unrelated active processes; preserve later note rename when finding historical witness. |
| deathrun | retrying; no upstream lead at baseline | Reconcile explicit curated image/report inclusions with non-security audit hygiene, preserving assets and stronger screenshot/machine-state rules. |

All intended destinations are resolved from the original logical targets and
actual configured aliases. Missing/ambiguous destinations stay unproven. No
remote was added or rewired to manufacture a matching name.

## Durable causes and corrections

### Daemon/runtime and observability

- Resolve installed Bun/pnpm roots before Tokio starts; interactive PATH success
  does not establish service availability. Installed-binary fixtures actually
  execute both through commit hooks with an initially incomplete PATH.
- Preserve independent publication, repository-local ownership, follow-up work
  during an existing push, bounded cancellation and capture lifetime. The large
  async capture buffer was moved off the debug stack after an actual overflow;
  concurrency, stack/deadline assertions were not relaxed.
- Preserve every ordinary commit lifecycle hook. A refused hook retains work and
  cannot commit/publish; repair and autonomous retry are exercised, not inferred.
- Match orphan process identity/ancestry before classification. Only independently
  identified abandoned prequalification fixture processes were signalled; active
  unrelated workloads were not killed.
- Keep absent scheduler evidence UNKNOWN with a reason, retain real retry/failure
  accounting and targeted remediation. A local operation deadline does not prove
  remote health. Explicit local hook worker-spawn EAGAIN no longer inherits the
  generic forge-outage budget exemption; the remote Gitaly counterexample remains
  transient. Generic hook failures now direct inspection of actual hook/product/
  environment output, not weakening storage policy.

### Hook/dependency and product integration

Browser gates execute each pushed Git revision in an isolated tree with its own
index, generated outputs and dependencies. Export attributes cannot omit/rewrite
inputs; source/dependency links cannot escape into the mutable checkout. Exact
Bun pins include a verified owned executable with canonical command names for
nested scripts. Libraries with dist entry points build before dependent
postinstall; normal scripted installation and npm pre/post lifecycle semantics
remain mandatory. Logs/receipts live in service-writable state, not inaccessible
PrivateTmp. Separate attempts retain distinct output and real exit codes.

Demonstrated product corrections include:

- Gemini toolkit: equivalent explicit C0 filtering instead of an illegal literal
  control-byte regex, with control/Unicode/fallback assertions retained. Repeated
  required-setting additions exposed a second, duplicated panel initializer;
  initialization now snapshots canonical defaults and clones its feature map.
  A private imperative prompt-insertion function was renamed rather than disabling
  React hook lint or changing its behavior.
- Clean Web: track only the exact historical public HN regression fixture with
  provenance; use existing representative tracked inputs for the leak probe.
  Today's different page failed the unchanged threshold and was rejected.
- Web Time Tracker/Nightfall: preserve source-first typed component declarations,
  actual Tailwind output and typed SVG navigation rendering. Positive and negative
  consumer/type/CSS regressions accompany the scoped source-type tracking rules.
- TabMon: repair demonstrated typing and provenance/tooling integration; recover
  exact historical sound prompts instead of inventing provider receipts. Its
  local navigation rule boundary follows declared WXT versus Kit framework
  identity, while genuine Kit navigation, injection and keyed-rendering checks
  remain enforced. This does not endorse unrelated asset-regeneration verdicts.
- Extension-core: one missing existing endpoint import in a subsequent catalogue
  addition refused the shared clean build and silently returned an empty list.
  The minimal import and mocked-fetch regression repair that defect without a
  provider call or feature expansion.

After product repairs, real service retries exposed native postinstall Tokio
EAGAIN/SIGABRT under the existing512task/twoCPU envelope. Shared gate child pools
are now bounded across library/install/lifecycle/product phases, with stricter
caller values preserved and effective limits recorded. No service task ceiling
was raised, deadline enlarged, lifecycle skipped or publication serialized.
A service-shaped private qualification passed both installs,7library builds and
12selected product gates on a5b450c29f836eac045a51f14a828e4edf3e16c8. This is
supporting diagnosis, not a substitute for the pending original whole hook.

Subsequent simultaneous mirror installs still exposed an aggregate task-budget
failure (Node startup uv_thread_create/SIGABRT). A repository-common-directory
OS lease now serializes ONLY dependency preparation, with a private validated
no-follow inode and finite300second acquisition. It releases before product
gates; no successful checks are cached/skipped, and other repositories, commits,
transfers and product gates remain independent. The21test suite proves exclusive
preparation, overlapping product gates, cancellation during preparation and lease
reacquisition. Cold-start test setup is separated from an explicit10second
post-SIGTERM join bound. Rejected timeout/startup qualifications remain retained.
Independent review found no issue; live multi-mirror validation remains pending.
The old476c295a cycle completed147gates with a dead-export failure already corrected
in newer source. A remaining mirror was still repeating that same immutable pin.
Only its isolated Git family was retired using pidfds after validating the failed
SHA, commands, UID, start identities, CWDs, cgroup and separation from the daemon.
No live family member remained; its manifest is cancelled, not passed. The daemon
and unrelated workloads were not signalled. Only this repository's stale retry
entry was then cleared; no executor push or ref/index/worktree mutation occurred.
Proof: checks/provisioning-lease/stale-operation-cancellation.json.

### Storage preparation and narrowly scoped hygiene

Existing-enrollment-only preparation avoids a reachable-history measurement
that cannot affect that branch's decision. New candidates still require the real
budget. Existing remote objects are still downloaded/readback verified, and every
reference receives final local-source revalidation; a concurrent edit refuses
publication and preserves both the edit and previous immutable object/manifest.
Original high-water ceilings and integrity assertions remain active.

The non-security audit correction honors only effective explicitly scoped file
inclusions. Blanket-equivalent globs are refused; screenshot, machine-state and
scratch rules remain stronger. Deathrun's curated image inclusions and eleven
exact report/reproduction/log inclusions are documented and behaviorally checked,
not blanket directory waivers. Files were not deleted/unstaged to hide the issue.

The parent later intentionally removed4446 obsolete top-level book samples.
4414 have identical retained canonical copies in HEAD/index/worktree;32 belong to
8slugs absent from both current catalogues in all views. Existing sanctioned
retiredRoots records only `static/books/*/chapters/*.md`. Canonical shelf paths,
other formats/deeper nonmatching paths, other applications and ref guards stay
protected. Old raw URLs are intentionally retired, not claimed preserved. The
full19test/75assertion forward-only suite and independent review passed before
and after adoption. No legacy history bytes were rewritten or reclaimed.

The separately authorized OpenAI detector correction aligns generated-hook
boundaries with the existing scanner and retains real credential detection,
including binary inputs. No filename allowlist, credential exemption or trust/
ownership policy relaxation was introduced.

## Qualification and actual execution

`checks/workspace-final-v6/` contains exact private source refs, unchanged required
smudge-filter configuration, normal locked Node SDK provisioning, raw commands
and results. Private bare refs/checkouts do not mutate canonical repositories;
compilation uses only an owned private cache, avoiding concurrent source/build
lock interference. Rust inputs are fingerprinted before/after. Full tracked-input
integrity additionally checks exact frozen-object bytes through required smudge
filters; eight equivalent historical/fixture files are recorded without retaining
plaintext content or plaintext hashes. No filter is disabled to clear this check.

- Locked workspace tests:2640passed,0failed,25previously ignored across41result
  suites. RUST_TEST_THREADS=1 serializes test cases, not the explicitly concurrent
  publication assertions. No default-parallel pass is claimed.
- Locked all-target strict Clippy:passed.
- Locked release build:passed.
- cargo-deny:passed without an advisory exception.
- Shared browser hook regressions:21passed; observer regressions and historical
  witness tests retain negative cleanup/alias/PID/object/budget counterexamples.

Earlier live-source and private-setup failures are retained, not reported as
passes: concurrent system edits, two nonblocking lease fixture failures, a new
sort idiom, missing Node SDK setup, and the classifier regression that revealed
the broad forge matcher. The serial harness passed the unchanged lease fixtures;
production leases/guards were not weakened. A controlled fork/CLOEXEC example
shows why a parent's close need not release an inherited pre-exec lease; it does
not claim the exact earlier failing PID was identified. The Clippy correction is
semantics-preserving, not implementation of that other worker's feature.

Both candidate and installed executable receipts pass against the final artifact.
They demonstrate blocked-A/independent-B publication, a second A commit and
latest-tip follow-up, actual tool/lifecycle execution, refused-work preservation,
repaired retry, repository ownership leases, joined shutdown and no leftover
fixture processes. All fixture remotes/HOME/state/watch roots are private and
outside real watch roots. The real service remained untouched during candidate
execution; deployment then used maintenance, backup, atomic replacement and
pidfd SIGTERM with Restart=always. `/proc/<pid>/exe` matches the qualified hash;
the maintenance marker was removed. No manual service stop was used.

## Proof semantics and remaining condition

A valid row requires the exact eligible blob in a post-baseline ancestor commit,
loaded-daemon PID/time-correlated commit output, and actual advertised descendant
refs advancing from before evidence on every intended destination. Continued
editing or a later note rename does not erase a previously published witness.
Failed Git queries never become an actual object ID. Correlation is expressly
not cryptographic author attestation.

Commit timestamps precede hooks. The old incidental30second correlation cap
falsely rejected the parent's31.613second hook execution. The current witnesses
record the observed effective60second stage-operation budget, verified against
qualified source plus absent operator/repo overrides. No production timeout was
increased. Tests still reject events outside that finite budget or from another
PID. Details: `checks/commit-event-correlation-budget.json`.

Contract rehearsal exposed two proof-generator gaps, not a false current row.
Generation now requires exact nonempty, distinct repository-set equality with
the independent initial inventory across witnesses and both snapshots. Logical
destination identities must remain stable; each alias must resolve to its claimed
normalized identity. Prior advertised commits are matched by destination, not
alias spelling. Empty/omitted/duplicate inputs and alias retargets fail closed;
transport spelling and extra-alias renames remain harmless. Eight contract tests,
five observer tests and eight historical-witness tests pass. The refreshed actual
same-runtime snapshot/journal still proves nine-of-ten, not browser success.
Evidence: `checks/ledger-contract-review/`.

Older pinned browser attempts correctly retained failures from subsequently
changed features: a missing Gemini welcome initializer (already repaired by its
owner), two typed DOM queries in a new chain fixture (minimal correction here;
current typecheck,458tests and lint pass), a centralized Auto Form Filler watcher
predicate and TabMon encounter-route/dead-export updates (already repaired in
newer source; current targeted16/58tests pass). Assertions were not disabled or
refreshed to claim those older snapshots passed. The next normal attempt must
validate corrected coherent source. Browser's refreshed eligible marker anchors
the provisioning repair, not merely an earlier source tip.

Ordinary attemptDy32YA on73e4dea subsequently caught new Gemini required-setting
and hook-name lint failures from later feature work. The canonical-defaults repair
above passes current package typecheck,502tests,lint and build. A prepared private
counterexample accepts an alternate canonical default and rejects the duplicated
initializer; no live source is modified by that experiment. Initial missing-WXT
setup runs remain rejected evidence. Independent narrow review found no issue.
Only the identity-revalidated, already-failed73e publication family was cancelled
so normal publication can select repaired source; it was never relabelled passed.
The refreshed browser eligible marker follows the actual repair anchor.
Evidence: `checks/gemini-canonical-defaults/`. Original full gates and remote
acknowledgments for the replacement source remain required.

**Still required:** terminal success of the original browser whole gates and
observed final-runtime eligible-tip advancement on all its intended remotes.
The daemon, not a manual push, must establish it. Current active work is not
called healthy or external. Subsequent unrelated feature changes are documented;
only demonstrated minimal gate/integration corrections were made.

Successful watched-repository commits and publication remain daemon-owned under
configured hooks. No real-repository manual recovery push/commit, history rewrite,
force push, destructive checkout operation or security-policy bypass was used.
Private test transports/refs are test fixtures, not recovery of a watched repo.
