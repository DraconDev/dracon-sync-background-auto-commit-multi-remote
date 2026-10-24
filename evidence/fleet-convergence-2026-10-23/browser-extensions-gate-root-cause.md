# browser-extensions-shared: why its own push gate refuses (root cause)

Read-only diagnosis. No edit to that repository, its hook, or its manifest was
made from this goal. This file records the demonstrated cause so the blocker is
described accurately instead of being reported as a sync or network fault.

## What the daemon sees

```
browser-extensions-shared | STALLED (1183 ahead) | last push: FAIL 7h
🔔 Push Stuck (budget exhausted): local pre-push hook refused the push
```

The classification is correct: the refusal comes from the repository's OWN
pre-push gate, not from the transport, the daemon, or the secret scanner.

`browser-extensions-shared` overrides `core.hooksPath` with
`scripts/git-hooks`, so the global warden pre-push hook fixed in this goal is
not the gate that blocks it. Phase 1 of that hook (the plaintext-secret scan) is
clean over the whole 1,200-commit backlog: a direct scan of every added blob in
`<remote-tip>..HEAD` matched zero times.

## The failing gate

```
scripts/git-hooks/pre-push -> run-push-gates.mjs
❌ Pre-push: 4-gate FAILED: snapshot dependency installation exited 1
 ERROR  Cannot find module
   .../snapshot-<sha>/extensions/collections/toolkit/chatgpt-toolkit/
   node_modules/@dracon/extension-core/dist/config/build.js
 error: postinstall script from "@dracon/chatgpt-toolkit" exited with 1
```

## Demonstrated cause

The gate exports the pushed revision into a disposable snapshot and runs:

```
bun install --frozen-lockfile --backend=copyfile --concurrent-scripts=1 --network-concurrency=8
```

`packages/extension-core` exposes its build entry through `dist/`
(`./build` -> `./dist/config/build.js`, with a `bun` condition pointing at
`src/config/build.ts`), and `dist/` is gitignored, so a fresh snapshot has no
`dist/`. `chatgpt-toolkit/wxt.config.ts` imports
`@dracon/extension-core/build`, and its `postinstall` is `wxt prepare`, so the
dependent's postinstall runs while that `dist/` is still missing.

`extension-core` declares `"prepare": "bun run build"`, which is exactly the
hook that would create `dist/` — but `--concurrent-scripts=1` serialises
workspace lifecycle scripts and the dependent's `postinstall` won, so
`prepare` had not run yet.

Reproduced twice from a clean export of the pushed revision (`HEAD`
`f4e38df5fe`), each time exiting 1 with the same missing module. Then, still in
the same export:

```
bun install --ignore-scripts            -> 0
bun run --cwd packages/extension-core build -> 0   (dist/config/build.js exists)
bun install --frozen-lockfile --concurrent-scripts=1 ... -> 0
```

So the snapshot is not broken and the lockfile is not stale: build order alone
decides the outcome. The same command succeeds as soon as `extension-core` is
built first.

## Scope correction and current execution

The earlier scope decision was incorrect: the confirmed objective explicitly
includes repairing demonstrated hook-integration and product-gate failures in
this initially affected repository, and prohibits deferring fixable backlogs.
This local lifecycle-order defect is in scope, not an external blocker.

The current runner already includes a two-phase frozen install: provision
without scripts, build dist-entry workspace libraries, then perform the normal
scripted install before all original product gates. The added real-Bun fixture
requires a dependent postinstall to resolve its built workspace library. On
2026-10-24 the installed runner and pre-push integration suites passed all 14
tests with zero failures (`node --test scripts/git-hooks/run-push-gates.test.mjs
scripts/git-hooks/pre-push.test.mjs`). A bounded real pre-push invocation on the
coherent revision recorded in `/tmp/fleet-browser-qualified/revision.json` is
running; this is gate execution only, not a manual push. Autonomous publication
remains unproven until that gate passes, targeted retries are re-enabled and
remote evidence is collected. No assertion or lifecycle hook was bypassed.
