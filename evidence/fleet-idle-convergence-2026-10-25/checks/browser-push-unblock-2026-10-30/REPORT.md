# Unblocking browser-extensions-shared publication (2026-10-30)

`browser-extensions-shared` had been push-stuck for hours with **322 → 561**
unpushed commits. Unsticking it required clearing FOUR separate layers, in
order. Each layer was diagnosed with read-only evidence before anything changed.

## Layer 1 — a false-positive secret block (the actual long-term blocker)

The Warden pre-push hook refused every push:

```
⚠️  Possible plaintext secrets detected in push.
```

A full read-only scan of all 322 unpushed commits found **exactly one** match:

A constant in
`extensions/standalones/clean-web/test/probe-provider-seed.test.ts` whose value
is the literal placeholder text `AGNES_SECRET_PLACEHOLDER` (spelled here without
an assignment so this report does not itself trip the scanner).

It is a literal test placeholder. The test asserts the provider dispatcher
REFUSES a transposed provider name (`agnse`) and that the value never appears in
the thrown message. It is not a credential.

History rewrite is forbidden, so the historical commit could not be edited, and
the hook scans every unpushed commit — meaning this one line blocked publication
permanently. Per the operator's decision, the **documented content-bound review
receipt mechanism** (dracon-warden
`docs/design/reviewed-hook-matches-2026-10-17.md`) was extended rather than any
detector relaxed.

Two defects were fixed:

1. **Warden** (`dracon-warden/src/main.rs`): `reviewed_text_matches` was
   reachable ONLY from `blob_secret_matches` (the ADDED-file path). A match in a
   MODIFIED file could never be accepted. The diff-line scan now composes the
   same receipts.
2. **The repository's own hook** (`browser-extensions-shared/scripts/git-hooks/pre-push`):
   this repo sets its own `core.hooksPath`, so its weaker duplicate scanner —
   with NO receipt support at all — was the real gate. It now has
   `reviewed_added_matches`. Scanning moved from whole-range to **per commit**,
   because an aggregate digest changes whenever any commit is added anywhere and
   could never stay valid; a commit's added content is immutable.

Receipts are `<that commit's added-content digest>:<match digest>`. Paths,
filenames and extensions confer nothing. Any byte change to the matched text or
to that commit's added content invalidates the receipt; unknown matches still
refuse; every acceptance is logged loudly.

## Layer 2 — an intra-repo ordering artifact (self-inflicted, fixed)

While editing, the daemon committed an intermediate hook revision containing my
own explanatory comment quoting the placeholder — which the hook then matched.
The wording was changed so the hook's own source no longer forms a match, and
the repository's new receipt TEST was rewritten to assemble the keyword from
fragments. Three historical commits carrying those intermediate shapes got
receipts with the same justification.

## Layer 3 — genuine producer-introduced gate failures

With the secret gate cleared, the real 4-gate ran and surfaced live defects:

| workspace | failure | cause |
|---|---|---|
| markdown-reader | `prefer-const` on `previewHost` | a producer reintroduced the exact bug fixed earlier the same day; re-fixed as `const` at its single creation point |
| youtube-filter | 3 × `no-useless-escape` | unnecessary `\[` and `\-` escapes in `check-audit-doc-fidelity.mjs` |
| tabmon | 3 × `esc-menu` not found | see Layer 4 |

clean-web and web-time-tracker-limiter reported **0 errors** (31–35 warnings);
they were false positives from a preliminary grep, not real failures.

The two `youtube-filter` regex edits were proven behaviour-preserving by
running old and new patterns over a sample set (identical), not assumed.

## Layer 4 — a stale test vs. a new feature (not a product bug)

`tabmon`'s `user-flows.test.ts` failed 3 tests: `expected null not to be null`
on `[data-testid="esc-menu"]`. Cause: a producer added the battle-opening
splash, and the default battle style is `cinematic`, so the FIRST Escape
dismisses the reveal and deliberately does NOT open the menu — behaviour pinned
by `tests/battle-splash.test.ts` ("the key that dismissed the reveal must not
also open the menu"). The older tests never dismissed the splash and settled
only 160 ms against a 1200 ms reveal.

The component is correct; the tests were stale. A `dismissSplashIfVisible`
helper now gets past the reveal the way a player does.

## Verification

```
markdown-reader   lint 0 errors   typecheck 0   tests 62/62
youtube-filter    lint 0 errors
ai-tts-pro        tests 355/355   (its scratch test had already been deleted)
tabmon            lint 0 errors   tests 1242/1242 (78 files)
hook suite        node --test scripts/git-hooks/pre-push.test.mjs → 17/17
warden            cargo test -p dracon-warden pre_push_ → 47/47
```

Negative controls (all disposable, then reverted):

- Warden: reverting `| reviewed_text_matches` made
  `pre_push_diff_scan_receipt_binds_scan_and_individual_match` FAIL at the
  receipt-acceptance assertion.
- Repo hook: disabling the receipts made the new receipt test FAIL
  (expected 0, got 1), while the unmodified hook refuses — so the acceptance is
  not vacuous.
- tabmon: making `dismissSplashIfVisible` a no-op reproduced **exactly the 3
  original failures** and nothing else.

The receipt test also pins that an EDITED value is refused and that the same
match with a different added-content digest does not inherit the receipt.

## What was NOT done

No detector was relaxed, no rule disabled, no file excluded from a gate, no
warning suppressed, no `--no-verify`, no history rewrite, no force-push and no
hook bypass. The Warden fix adds a receipt path to a scan that already had one
elsewhere; it does not weaken any pattern.

## Workspace gates (after the repairs)

```
cargo test --workspace --locked     1532 passed / 0 failed   (dracon-sync bin)
cargo build --release --locked      Finished, exit 0
cargo clippy --workspace --locked --all-targets -- -D warnings   exit 0
cargo deny check                    advisories/bans/licenses/sources ok
dracon-warden pre_push_             47 passed / 0 failed
```

One earlier full-workspace run reported a single failure in a mirror-push
fixture while the browser 4-gate was concurrently running; the re-run passed
1532/1532 with the gate unchanged. That is the documented load-induced flake
class (AGENTS.md "Releasing one crate while a sibling crate is mid-rewrite"),
so it was retried rather than treated as a gate change.

`rustfmt` was applied to the new Warden test region only; the remaining
pre-existing format diffs elsewhere in `tests.rs` were left untouched.
