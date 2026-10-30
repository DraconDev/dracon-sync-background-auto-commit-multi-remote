# Two producer-introduced `prefer-const` gate failures (2026-10-30)

## Symptom

`browser-extensions-shared` publication stalled again after the earlier
`no-useless-escape` repair, this time on two **different** lint errors in files
its own producers had just written:

```
extensions/standalones/markdown-reader-LINT.log
  entrypoints/options/main.ts
    297:7  error  'previewHost' is never reassigned. Use 'const' instead  prefer-const
  ✖ 22 problems (1 error, 21 warnings)

extensions/standalones_ai-chat-toolkit-LINT.log
  scripts/acceptance.ts
    64:9   error  'timer' is never reassigned. Use 'const' instead  prefer-const
  ✖ 1 problem (1 error, 0 warnings)
```

The other three gates for both workspaces passed in the same run, so the 4-gate
hook was correct: lint was the sole blocker, and the daemon went push-stuck
(5 consecutive failures, 240 unpushed commits) with the accurate cause
`local pre-push hook failed`.

## Why the obvious fix is wrong

Both symbols are read by a **closure that can run before the single later
assignment**. Applying the linter's suggestion literally (`let` → `const` in
place) puts the closure in the temporal dead zone:

```
$ node  # naive const
timer, child already exited -> TDZ:ReferenceError
```

That is not hypothetical for `acceptance.ts`: `finish()` is reachable from
`onExit` and from the two post-assignment `exitCode`/`signalCode` checks, and
`waitForChildExit` returns early for an already-exited child — the exact case a
headless-Chrome acceptance driver hits when Chrome dies immediately. For
`main.ts`, `savePatch` closes over `previewHost` and is wired into the settings
form before `previewHost` is created.

So `prefer-const` cannot be satisfied by the literal edit; the code has to be
restructured to have one real initialization that every reader sees.

## Repair

**`ai-chat-toolkit/scripts/acceptance.ts`** — hoist the readers to function
declarations (hoisted, so callable before the `const`) and create `timer` once:

```ts
function finish(exited: boolean) { clearTimeout(timer); child.off('exit', onExit); resolve(exited); }
function onExit() { finish(true); }
const timer = setTimeout(() => finish(false), timeoutMs);
timer.unref();
```

**`markdown-reader/entrypoints/options/main.ts`** — drop the premature `let` and
declare `previewHost` as a `const` at its single creation point, which is still
before every read (`savePatch` only runs after the form is wired):

```ts
const previewSection = sectionId(section('Preview'), 'preview');
const previewHost = preview(settings);
```

Semantics are unchanged; only the declaration form and the ordering of the
closure definitions change.

## Verification

Correctness of the restructure, checked in isolation rather than assumed:

| shape | naive `const` | applied fix |
|---|---|---|
| reader runs before assignment (`child already exited`) | `ReferenceError` (TDZ) | `true` (correct) |
| child exits first | — | `true` (correct) |

Live gates in the real worktrees:

```
markdown-reader   lint exit 0 → ✖ 22 problems (0 errors, 22 warnings)   typecheck exit 0   tests 62/62
ai-chat-toolkit   lint exit 0                                            typecheck exit 0   tests 443/443
```

Zero errors in both; the remaining warnings are pre-existing
`no-explicit-any`/`no-unused-vars` findings. No lint rule was disabled, no file
was excluded from the gate, and no warning was suppressed.

## Scope honesty

This clears the two **current** blockers. It is necessarily a moving target: the
produces keep writing, and a further lint error can appear in the next snapshot.
That is normal gate behaviour (the hook is gating real work), not a daemon
defect, and no gate was weakened to make publication easier. No regression test
was added for these two files because both are entrypoint/CLI scripts with no
test harness of their own — `acceptance.ts` is a top-level driver with no
exports and `main.ts` is a WXT entrypoint — so the behaviour was verified by the
isolated TDZ controls above plus the live lint/typecheck/test gates rather than
by fabricating a test surface that does not exist.
