# markdown-reader lint refusal — repair (2026-10-29)

## Symptom

`browser-extensions-shared` publication was refused for **seven consecutive
gate runs** by the same LINT failure, not by a flake and not by the network:

```
extensions_standalones_markdown-reader-LINT.log
  ✖ 24 problems (7 errors, 17 warnings)
    error  Unnecessary escape character: \[  no-useless-escape   (adoc.ts 20:35, 24:28)
    error  Unnecessary escape character: \[  no-useless-escape   (dialects/index.ts 16:26)
    error  Unnecessary escape character: \/  no-useless-escape   (org.ts 23:32)
    error  Unnecessary escape character: \-  no-useless-escape   (frontmatter.ts 21:28)
    error  Unnecessary escape character: \^  no-useless-escape   (inline-marks.ts 23:49)
    error  Unnecessary escape character: \+  no-useless-escape   (inline-marks.ts 25:51)
```

All three other gates for that workspace passed in the same run (BUILD ✓,
TYPECHECK ✓, TEST 57/57 ✓), so the 4-gate hook was behaving correctly: the
lint gate was the sole, real, reproducible blocker. Gate manifests
(`dracon-pre-push.LyqohC`, `.xNjpDK`, `.yX25xv`, `.2j1UBZ`, …) each name
`extensions/standalones/markdown-reader → failed`, which in turn keeps
`browser-extensions-shared` in the 15-minute backoff class and lets its
unpushed backlog grow (journal: 292 → 303 while observed).

## Root cause

Six regex character classes were written with a needless escape on a character
that ESLint's `no-useless-escape` (enabled by `js.configs.recommended` in the
repo's flat config) treats as unnecessary inside a `[...]` class:

| file | before | after |
|---|---|---|
| `lib/dialects/adoc.ts:20` | `[^\s\[\]]` | `[^\s[\]]` |
| `lib/dialects/adoc.ts:24` | `[^\[]` | `[^[]` |
| `lib/dialects/index.ts:16` | `([\\`*_{}\[\]()#+\-.!\|>~$])` | `([\\`*_{}[\]()#+\-.!\|>~$])` |
| `lib/dialects/org.ts:23` | `[^\/\n]` | `[^/\n]` |
| `lib/markdown/frontmatter.ts:21` | `[A-Za-z0-9_.\-]` | `[A-Za-z0-9_.-]` |
| `lib/markdown/plugins/inline-marks.ts:23` | `[^\^\n]` | `[^^\n]` |
| `lib/markdown/plugins/inline-marks.ts:25` | `[^\+\n]` | `[^+\n]` |

The repair removes exactly the redundant backslash character in each class and
changes nothing else. Two invariants are deliberately preserved and are the
reason this is not a mechanical find-and-delete:

- the **dash** stays the LAST character of `[A-Za-z0-9_.-]` so it remains a
  literal, not a range;
- `[` and `]` both stay escaped inside the escape class, so bracket escaping in
  `plainTextToMarkdown` is unchanged.

## Equivalence check (why this is regex-preserving, not behavior-changing)

For each of the seven pairs, old and new patterns were executed over 91,003
generated samples (all single/double/triple combinations of the classified
characters, the delimiters, URL/label shapes, and `key<chars>: value` forms) and
the full match arrays were compared. **637,021 pattern/sample combinations, zero
mismatches** — the removal is a source-level escape cleanup, not a semantic
change.

## Regression coverage added

`tests/dialects.test.ts`, `tests/frontmatter.test.ts` (commit `43d316ca07`):

- `converts image macros even when the path or alt text contains brackets`
- `converts italic runs whose content contains no slash`
- `still escapes brackets now that the escape class is not over-escaped`
- `accepts dots, dashes and underscores in keys`

Each protects an intentional contract (bracket/URL parsing, dash-as-literal,
bracket escaping), not an incidental total.

## Disposable controls (isolated copy, live tree untouched)

A copy of the workspace member at `/tmp/mrneg/extensions/standalones/markdown-reader`
(with the repo's `packages/`, `node_modules/`, `tsconfig.json` reachable through
the same relative layout) was used for negative controls:

| control | code under test | result |
|---|---|---|
| harmless change (the actual fix) | escapes removed | **61 passed / 0 failed** — the new assertions tolerate it |
| pre-fix source | original over-escaped classes | 61 passed / 0 failed — the *old* code was still functionally correct, which is precisely why the lint gate, not a test, was the blocker |
| careless fix A | dash/brackets **deleted** from the classes | **2 failed** (`accepts dots, dashes and underscores in keys`, `still escapes brackets …`) |
| careless fix B | dash kept but as a **range** | **2 failed** (same assertions) |

So the added coverage accepts a behavior-preserving cleanup and rejects the two
plausible wrong repairs a lint chaser would make.

## Live verification after the repair

Run in the real worktree (`extensions/standalones/markdown-reader`):

```
bun run lint        → exit 0   (✖ 17 problems, 0 errors, 17 warnings)
bun run typecheck   → exit 0
bun run test        → exit 0   (4 files, 61 tests passed)
```

Raw logs: `live-lint-after.log`, `live-typecheck-after.log`,
`live-tests-after.log` in this directory.

## Scope honesty

- This resolves the demonstrated **lint** refusal for `markdown-reader`. It does
  not by itself establish that `browser-extensions-shared` published; the
  4-gate hook must still run on a revision containing the fix, and the daemon
  must push it. A new gate run (`dracon-pre-push.624gNQ`, started 18:43:33Z)
  has no failing gate so far at the time of writing — a live observation, not a
  final result.
- Warnings (`@typescript-eslint/no-explicit-any`, 17) are pre-existing and are
  not errors; they were not silenced, and no rule was disabled or weakened.
- No hook, gate, assertion or lint rule was bypassed, relaxed or skipped, and
  the fix was committed through the daemon's normal auto-commit path.
