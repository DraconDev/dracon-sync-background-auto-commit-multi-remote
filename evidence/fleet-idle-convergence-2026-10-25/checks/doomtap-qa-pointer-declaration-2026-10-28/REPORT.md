# Doomtap: staged-intent deferral caused by an undeclared asset-pointer class

## Symptom

`doomtap` could not commit at all. The journal repeated:

    ⚠️ sync failed (late) for .../web/games/wip/doomtap: refused epoch message
       retained at .../dracon-epoch-message-XXXX

with 141 retained epoch directories accumulating. Every staged epoch was
retained instead of committed, so the repository never converged.

## Root cause

The fleet pre-commit's machine-local staging guard (`~/.config/git/hooks/pre-commit`)
refuses any staged path under `docs/screenshots/*` unless it can be classified
as a visual SOURCE via an exact root rule. Its inclusion script requires:

    case "$pattern" in
        docs/screenshots/*) visual_source=1 ;;
        *) exit 1 ;;
    esac

and it deliberately rejects a broad type glob ("a broad !*.png must never turn
frame caches into sources").

The single staged file was `docs/screenshots/qa/trophy-before.png`. The only
`.gitignore` rule matching it was the managed broad `!*.png`, which the guard
rejects — so the path could never be classified and every commit was refused.

The crucial detail: in this repository `docs/screenshots/qa/*.png` is NOT a
frame-cache tree. `*.png` is routed through the `dracon-assets` L2 filter
(`.gitattributes`: `*.png filter=dracon-assets`), so what git stores for a qa/
frame is a small `version dracon-asset-pointer/v1` record naming the sha256 of
the real bytes in the asset store. Eight such pointers were ALREADY tracked in
HEAD (`gamefeel-*`, `skillbar-cooldowns`, `hero/audit-hero-*`) and
`docs/AUDIT-LEDGER.md` cites them as the evidence for real verdicts — the
2026-10-28 Trophy Room entry cites exactly this pair:

    docs/screenshots/qa/trophy-before.png → docs/screenshots/qa/trophy-after.png

So the blocker was a MISSING POLICY DECLARATION for a class of content the
repository deliberately versions, not a bad file.

## The fix

Exact root declarations in the repository's own `.gitignore`, one named file
each — no directory glob, no `**`, no type glob:

    !docs/screenshots/qa/gamefeel-game-kill-fx.png
    !docs/screenshots/qa/gamefeel-prestige-panel.png
    !docs/screenshots/qa/gamefeel-upgrades-panel.png
    !docs/screenshots/qa/hero/audit-hero-composite.png
    !docs/screenshots/qa/hero/audit-hero-source-arena.png
    !docs/screenshots/qa/hero/audit-hero-source-title.png
    !docs/screenshots/qa/hero/audit-hero-source-town.png
    !docs/screenshots/qa/skillbar-cooldowns.png
    !docs/screenshots/qa/trophy-before.png
    !docs/screenshots/qa/trophy-after.png

Style note: declarations use `!path` (matching the curated block above them);
the QA-cache IGNORE rules below use `/path`. The hook strips an optional
leading `/` from the pattern, so both are accepted, but the repository's
convention distinguishes them and the policy test asserts each shape.

This grants permission to version those ten pointers. It does not re-open the
qa/ tree, and the curated comparison contract is unchanged.

## Verification

Policy test — `scripts/lib/visual-input-publication-policy.test.mjs`, now
4 tests (a new one was added for this class):

    ✔ publication names match the original visual comparison contract exactly
    ✔ approved QA caches are ignored by exact rules, outside the unchanged source contract
    ✔ versioned QA asset pointers are declared by exact rules, not by the broad PNG glob
    ✔ harmless comments pass; omissions, broad exemptions and unknown frames fail
    pass 4 / fail 0

The new test asserts, for each of the ten pointers, that it is declared by its
OWN exact rule and that `git check-ignore --verbose` resolves it to `!<path>` —
never the broad `!*.png`. It also asserts `exactContract(rules)`, so the
curated comparison set cannot be altered as a side effect.

Positive control — the repository's own hook:

    $ ../../../scripts/git-hooks/bucket-hooks/pre-commit
    hook_rc=0

Negative control — the guard must still refuse an UNAPPROVED qa/ frame. A
probe file `docs/screenshots/qa/probe-unapproved.png` was staged and the hook
run:

    machine-local file is staged: docs/screenshots/qa/probe-unapproved.png
    pre-commit: machine-local staging policy blocked this commit.

So the fix permits the declared pointers and still blocks the class it did not
declare. The probe was removed from the index and the worktree afterwards.

## Live result

Doomtap commits continuously again through the daemon (no manual push):

    656fad26f  33 file(s) in assets,docs,scripts ... NEW:2026-10-28-trophy-hall/
    c6b05d143  2 file(s) in docs [.gitignore] ... NEW:qa/trophy-after.png
    42b3b6623  2 file(s) in scripts [...policy.test.mjs, .gitignore]

954 commits in the 20 minutes after the fix (the repository had been unable to
commit at all), `origin` 0 ahead, working tree clean.

The retained `dracon-epoch-message-*` directories continue to accumulate; they
are the daemon's epoch spool, not the refused epochs — the refused-epoch
messages stopped appearing for doomtap after the fix.
