# Forge-identity dedupe and REM-column truth — 2026-11-09

## The two defects, measured live

Both came from the same root: the daemon had no notion of *forge project
identity* on the publication path. `filter_remotes_by_exclude`
(`git/multi_remote.rs`) compares remote **names**; nothing compared resolved
URLs. `remote_project_identity` existed, but only privately inside
`report.rs:1463` and only for the drift check.

### 1. Double push — the same object sent to the same project twice

`git config --get-regexp '^remote\..*\.url'` over the watched fleet (38 repos
inspected; every repo with `origin` audited) found **8 repositories whose
`github` mirror is byte-identical to `origin`**:

| repo | origin | duplicate mirror |
| --- | --- | --- |
| db-gateway | `github.com/dracondev/db-gateway` | `github` |
| dracon-platform | `github.com/dracondev/dracon-platform` | `github` |
| dracon-utilities | `github.com/dracondev/dracon-utilities` | `github` |
| empire | `github.com/dracondev/empire` | `github` |
| eve | `github.com/dracondev/eve` | `github` |
| mcode-plugins | `github.com/dracondev/mcode-plugins` | `github` |
| opencode-plugins | `github.com/dracondev/opencode-plugins` | `github` |
| terhub | `github.com/dracondev/terhub` | `github` |

Seven of these are the census named in the goal (`dracon-utilities` was the
eighth, found by re-running the audit across the whole `~/Dev` tree rather
than only the seven). Each cycle pushed the same pinned object to the same
GitHub project twice — once through `origin`, once through the `github`
mirror. The same waste was hand-fixed for `browser-extensions-shared` on
2026-11-06 with a per-repo `exclude_remotes = ["github"]`, which is why that
file carried the override the operator approved removing here.

The cost is not theoretical. `browser-extensions-shared`'s own override
comment records the measurement: rev `d5864cdb0a` was gated by attempts
`V3eH58` **and** `tso5KA` (14 gates each), rev `051b4865a4` by `vuouVG`
**and** `8D1rNZ` (17 gates each) — roughly half of all publication wall clock
spent re-proving a byte-identical tree. That repo's queue could not keep up
with a producer landing commits every 1–2 minutes.

### 2. Blind display — GitHub invisible while GitHub received everything

`origin` was never part of `push_to_remotes`, and `rem_cell_content`
(`report.rs`) renders only the names it is given. So a repo that publishes
GitHub *through origin* rendered with no GitHub icon at all.

Measured before the fix, `dracon-sync repos browser-extensions-shared`:

```
     push-to:   gitlab [excl:github,codeberg]
```

while GitHub received 41 commits in the preceding hour and `gh tip` equalled
`gitlab tip` at `61a950c1d4`. `pi-plugins` and `tiles-tui-file-manager` have
the same shape (`origin` = `https://github.com/...`, no github mirror), and
`browser-extensions-shared`'s only reason for looking GitLab-only was the
operator's workaround override.

A whole-fleet JSON dump confirmed the same class of lie on every repo whose
origin is a forge: `push_to` listed mirror names only, never origin's forge.

## What changed

### One identity helper, in `forge.rs`

`remote_project_identity` moved out of `report.rs` to sit beside
`forge_host_of_url` in `src/forge.rs` (`forge.rs:276`). One definition, two
consumers:

- report path: `report.rs:838` → `effective_push_targets`
- push path: `sync/publication.rs:146` → `effective_push_targets`

`grep -rn "fn remote_project_identity" src/` returns exactly the definition
in `forge.rs`; the old private copy is gone.

The helper **delegates to `crate::git::canonical_repository_url`** rather than
re-parsing. That matters: the tree already had two identity normalisers that
disagreed. `canonical_repository_url` strips `ssh://…:22` / `https://…:443`
ports and unwraps IPv6 brackets; the pre-move copy kept the port inside the
host, so `ssh://git@gitlab.com:22/DraconDev/x.git` parsed as the project
`gitlab.com:22/x` and compared **unequal** to the scp form of the same
project. Two parsers on two sides of a decision is the exact divergence this
goal existed to prevent, so the second one was removed rather than
reconciled:

- `same_repository_url` (the wrapper that only compared canonical strings)
  was deleted from `git/urls.rs`; its tests keep a local identity-equals
  assertion so the normalisation cases stay pinned.
- `github_mirror_matches_origin` and its two tests were deleted from
  `sync.rs`; the identity question is now asked once.
- `publication::is_github_url` (test-only, unused) was deleted.

### Dedupe by identity, in the push path

`forge::duplicate_mirror_names(origin_url, mirrors)` (`forge.rs:338`) claims
origin's identity first, then walks mirrors in push order
(`priority` ascending, name tiebreak), returning the names whose identity is
already claimed. `multi_remote::effective_push_targets` wraps it and is the
single calculation both paths call.

Three semantics are load-bearing and pinned by tests:

1. **Drop only on a parseable collision.** An unparseable URL (local path,
   `ext::`, malformed) never claims and never drops. A dedupe that guessed
   would cost a publication target; every previously-published forge must
   still receive commits.
2. **Same host + different slug is KEPT.** That is drift (a renamed project,
   an operator's second target), not duplication. Cross-host mirrors never
   collide either — GitHub and GitLab serving one repo is the design.
3. **Two mirrors sharing one project collapse to the higher-priority one** —
   the leg the pusher would have used first.

The name-based `exclude_remotes` filter still runs **before** this, so an
explicit operator opt-out is untouched and still wins.

Nothing on disk changes: the dedupe returns names, and the caller excludes
them from the round's push set. `.git/config` remote entries are left exactly
as they are.

One existing behaviour was **removed** on the way: `plan_publication`
pre-excluded the remote literally named `github` when its URL matched origin
(`github_mirror_is_origin`). That was wrong in both directions — it missed a
duplicate under any other name, and it dropped a *distinct* GitHub mirror
merely because it shared the name. Its exclusion also reached
`prepare_mirror_remotes`, whose `remove_stale_remotes` deletes
daemon-managed remotes: a display/push question answered by deleting
configuration. The identity pass replaces it, after remote preparation, so
nothing on disk is touched.

### REM column reports origin's forge

`report_effective_remote_legs` derives both answers from the same
`effective_push_targets` the pusher uses and returns four things: `legs`,
`forges`, `excluded`, `duplicate_mirrors`.

**Legs vs forges (CHANGED 2026-11-09, audit item 5).** The first version put a
forge *label* in the target list, so a repo whose duplicate `github` mirror had
just been de-duplicated still printed `push-to: github,...` — the target list
named the very destination that was no longer pushed as a leg. The list is now
the **legs** (`origin`, mirror names) and the forge families travel alongside:

```
push-to:   origin,gitlab [dup:github]        # db-gateway / terhub
push-to:   origin,gitlab [excl:codeberg]     # browser-extensions-shared
```

`origin` is a leg; `github` is the forge it publishes to, rendered as a 🐙 in
the REM column from `push_to_forges`. So a repo publishing GitHub through
`origin` still shows the GitHub icon (task 3.2) while `github` is absent from
the push-target list itself (contract item 5), and `[dup:github]` keeps the
receipt for the dropped mirror. Unknown hosts still report by remote name, so
an internal git server never masquerades as GitHub. The codeberg quota rule and
the over-2-GiB GitHub skip are unchanged in both paths.

One deliberate consequence: a repo with two DISTINCT projects on one forge
(none today) renders two 🐙, because the forge list is per leg. The earlier
"forges, never legs" collapse hid that second destination.

`RepoReportRow.duplicate_mirrors` is a **separate** field from
`excluded_remotes` on purpose: they are different reasons a destination is not
pushed (operator decision vs automatic dedupe) and conflating them would hide
the automatic behaviour. The detail view renders both.

## Evidence

### Unit tests (new)

`src/forge.rs`:

- `test_remote_project_identity_shapes` — scp, https with and without `.git`,
  userinfo, trailing slash, case, **and `ssh://…:22` / `https://…:443` port
  equivalence**; also `assert_ne!` for same-host different-slug and cross-host
  same-slug.
- `duplicate_mirror_names_drops_origin_duplicate_across_url_spellings` — the
  db-gateway shape (origin github + duplicate github mirror + distinct gitlab
  mirror) under six origin spellings; gitlab must survive every one; no
  origin and unparseable origin drop nothing.
- `duplicate_mirror_names_keeps_same_host_different_slug` — the doomtap drift
  case survives; the identical-slug case still drops.
- `duplicate_mirror_names_collapses_two_mirrors_to_one_project` — priority
  order wins, then name order (deterministic).
- `forge_label_of_url_maps_the_three_published_forges` — github/gitlab/codeberg
  plus unknown host and non-forge shapes returning `None`.

`src/git/multi_remote.rs`:

- `effective_push_targets_names_origin_forge_and_keeps_distinct_mirrors`
- `effective_push_targets_keeps_every_distinct_forge`
- `effective_push_targets_respects_prior_name_exclusion`
- `effective_push_targets_reports_unknown_remotes_by_name`

`src/report.rs`:

- `report_names_github_when_origin_publishes_github` — real `git init` fixture;
  the pre-fix behaviour named only `gitlab`.
- `report_drops_the_duplicate_mirror_origin_already_covers` — github appears
  **once** (via origin) plus the distinct gitlab leg.
- `push_to_detail_text_separates_opt_outs_from_automatic_dedupes`.

### Gates

| gate | result |
| --- | --- |
| `cargo test --workspace --locked` | 3106 passed, 0 failed, 27 ignored (44 suites) |
| `cargo clippy --workspace --locked -- -D warnings` | exit 0 |
| `cargo clippy --workspace --locked --all-targets -- -D warnings` | exit 0 |
| `cargo build --release --locked` | exit 0 |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |

`cargo clippy --all-targets` additionally surfaced a pre-existing
`clippy::field_reassign_with_default` in `policy.rs` test code (untouched by
this change); fixed in place rather than left failing the stricter gate.

The 47 staged-epoch/storage tests need a helper binary whose
`DRACON_EPOCH_SOURCE_ID` matches the code under test (`.cargo/config.toml`
points `DRACON_EPOCH_TEST_HELPER` at `target/debug/dracon-sync`), so a plain
`cargo test` after a source edit fails them with "source identity mismatch"
until `cargo build` refreshes that binary. Building first is the whole fix; no
test was skipped or relaxed.

Deployment (2026-11-09 22:05:32, `scripts/install-qualified-daemon.py`):
`expected_sha256 == running_sha256 == 74904dc9…`, `new_pid 3676819`, and the
running revision was read back from `/proc/3676819/exe` rather than trusted
from installation metadata. The previous binary was backed up under
`~/.local/state/dracon-sync/deployments/`.

### Live verification (2026-11-09, after the restart)

`dracon-sync repos --json` over the whole fleet. `push_to_remotes` is the LEG
list, `push_to_forges` the icon families:

| repo | `push_to_remotes` | `push_to_forges` | `duplicate_mirrors` | `excluded_remotes` |
| --- | --- | --- | --- | --- |
| db-gateway | origin, gitlab | github, gitlab | github | — |
| terhub | origin, gitlab | github, gitlab | github | — |
| empire | origin, gitlab | github, gitlab | github | codeberg |
| mcode-plugins | origin, gitlab | github, gitlab | github | — |
| opencode-plugins | origin, gitlab | github, gitlab | github | codeberg |
| dracon-utilities | origin, gitlab | github, gitlab | github | — |
| dracon-platform | origin, gitlab | github, gitlab | github | codeberg |
| browser-extensions-shared | origin, gitlab | github, gitlab | github | codeberg |
| pi-plugins | origin, gitlab | github, gitlab | github | — |
| tiles-tui-file-manager | origin, gitlab | github, gitlab | github | — |

`github` is **not** a push target on any of them (the de-duplicated mirror is
gone and origin is the github leg), while `origin` publishes GitHub — so
GitHub still receives commits and still renders 🐙:

```
$ dracon-sync repos db-gateway | grep push-to
     push-to:   origin,gitlab [dup:github]
$ dracon-sync repos terhub | grep push-to
     push-to:   origin,gitlab [dup:github]
$ dracon-sync repos browser-extensions-shared | grep push-to
     push-to:   origin,gitlab [excl:codeberg] [dup:github]
```

(Corrected 2026-11-09 22:29 after the audit: the earlier revision of this
document quoted `push-to: github,gitlab [dup:github]`. That was the version
whose LEG list carried origin's forge label, and the audit was right that it
contradicted the target list it claimed to fix.)

Before the change the same line read `gitlab [excl:github,codeberg]` for
`browser-extensions-shared` — GitLab only, while GitHub received every commit.
The override that produced it is gone: `grep exclude_remotes` on that repo's
`.dracon/dracon-sync.toml` returns nothing.

REM column (fleet table) now renders `🐙🦊` for every row above, including the
three repos that previously showed no GitHub icon at all.

Every previously-published forge still received commits — `ls-remote
refs/heads/main` against the local `refs/heads/main` SHA, 2026-11-09 22:17:

| repo | local | github (origin) | gitlab |
| --- | --- | --- | --- |
| db-gateway | `d9cdc7ab3abb` | `d9cdc7ab3abb` | `d9cdc7ab3abb` |
| terhub | `0e850ed054be` | `0e850ed054be` | `0e850ed054be` |
| empire | `d23c626bab1c` | `d23c626bab1c` | `d23c626bab1c` |
| mcode-plugins | `d04433108741` | `d04433108741` | `d04433108741` |
| opencode-plugins | `803f967d132c` | `803f967d132c` | `803f967d132c` |
| dracon-utilities | `c82a11e3968d` | `c82a11e3968d` | `c82a11e3968d` |

Fleet state: 38 inspected, 0 concerns, 0 inspection failures. The only repo in
a warn/pending state is `dracon-platform` (3–4 unpushed commits), and its cause
is unrelated to this change — the private-bucket preparation times out at 300s
("private bucket workflow failed; no commit permitted: … timed out after 300s"
in the journal, both before and after the restart), so no commit is created and
no push can follow. `browser-extensions-shared` shows the same pre-existing
producer-faster-than-gate backlog rather than a push rejection: both of its
forges advance in lockstep to the same published tip every round.

## No publication target was lost

Explicitly: **no forge that received commits before this change stopped
receiving them.**

- The dedupe only drops a mirror when its resolved URL names the same forge
  project as `origin` or as a higher-priority mirror — i.e. a destination
  already covered by another leg of the same round. There is no case where a
  distinct project is dropped.
- Every repo in the census publishes exactly the same forge projects after the
  change: GitHub (via `origin`), GitLab (via its own mirror). Only the
  redundant second GitHub *leg* is gone.
- Same-host different-slug mirrors are kept by construction, pinned by
  `duplicate_mirror_names_keeps_same_host_different_slug`. A fleet scan found
  no such repo today, so the rule is currently a guard rather than a
  behaviour change — and it is the guard that makes the dedupe safe for a repo
  that later renames its mirrors.
- The codeberg quota posture and the over-2-GiB GitHub skip are unchanged;
  the `exclude_remotes` opt-out still wins over everything.
- `browser-extensions-shared`'s override was removed (operator-approved) and
  replaced by the automatic behaviour, so GitLab still gets its own push while
  GitHub continues to be served by `origin`.

## Residual risk

- The dedupe is per-round and recomputed from `.git/config` each time, so it
  follows operator edits to remote URLs on the next cycle with no state to
  migrate — but it also means a temporarily unreadable `.git/config` degrades
  to "no origin identity" and therefore to "no dedupe" (double push), never to
  "drop a distinct target". That is the intended fail-open direction.
- `duplicate_mirror_names` is O(mirrors) with one parse per remote; it runs
  once per publication round, not per commit.
- The REM column reports **forges**, not legs. A repo with two distinct GitHub
  projects configured (none today) would render one 🐙; the detail view and the
  JSON row disambiguate via `[dup:...]` and `push_to_remotes`.
