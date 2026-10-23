# Why `browser-extensions-shared` is not converged (honest diagnosis)

The objective requires distinguishing active edits, waiting pushes, failed gates
and external blockers rather than reporting everything as healthy. This file is
that distinction for the one repository that stays `warn`/`PENDING`.

## Observed state

```
browser-extensions-shared | PENDING | 18 unpushed commits | 4-gate FAILED
```

The daemon log names the gate precisely:

```
❌ Pre-push: 4-gate FAILED for:
  - extensions/standalones/youtube-filter/TEST exited 1
  - extensions/standalones/youtube-filter/TYPECHECK exited 2   (one attempt)
```

So this is a **product gate refusal**, not a push/network/daemon defect. The
daemon is behaving correctly: it classified the refusal as repository policy
(its own alert text says "local pre-push hook refused the push (repo policy:
asset-guard / size budget / secret scan / history guard — fix the policy, not
the network)"), kept the commits, and kept retrying.

## Root cause of the observed TEST refusal

I reproduced it:

```
tests/store-assets.test.ts:89
expect(shotDashboard).toContain(`"${label}"`)
Test Files  1 failed | 53 passed (54)
Tests  2 failed | 896 passed (898)
```

`store-assets.test.ts` reads two files at module load:

```ts
const generator = readFileSync(resolve(root, 'generate_store_assets.py'), 'utf8');
const manifest  = JSON.parse(readFileSync(resolve(root, 'store-assets/ASSETS.json'), 'utf8'));
const shotDashboard = generator.slice(generator.indexOf('def shot_dashboard'));
```

Its assertions are `toContain` checks against **the literal source text of
`generate_store_assets.py`**. Another agent was editing that exact file while my
run was reading it:

```
11:37  ASSETS.json rewritten (caption changed)
11:43:02  my test run reads generate_store_assets.py  -> 2 failures
11:43  ba7e7d4865  generate_store_assets.py + ASSETS.json changed
11:44  1e52d1467c  store-assets.test.ts changed (17 lines)
11:45  473a192091  workspace docs changed
```

That is a **live-edit race against a source-text pin**, not a product defect.
Re-running against the settled files:

```
npm run test      -> Test Files 54 passed (54) | Tests 899 passed (899)
npm run typecheck -> exit 0
```

Twice, back to back, plus the isolated file (`8 passed`).

## What this is and is not

* It IS a real, reproducible gate failure caused by concurrent modification of
  the inputs the gate reads.
* It is NOT a dracon-sync defect, NOT a weak/skipped assertion, and NOT
  something to "fix" by loosening the pin.
* It is NOT within this goal's remediation scope: the objective bounds product
  remediation to failures established in the initial diagnosis and regressions
  caused by this work. This failure appeared at 11:43 **during** verification,
  caused by a different agent's edits in another repository.
* It is NOT reported as converged. The repository is reported as a genuinely
  gated backlog.

## Why I did not "repair" it

The test pins generator source text. Editing either the generator's labels or
the test's expectations to make a transient race pass would be exactly the
coverage weakening the contract forbids, and would collide with the agent
actively editing that workspace. The correct resolution belongs to whoever is
authoring `youtube-filter`'s store assets; the daemon correctly keeps the
commits safe and retries until the gate passes on settled content.

## Consequence for the fleet observation

`browser-extensions-shared` may remain PENDING for a while. That is the honest,
correct outcome of a refusing product gate; the alternative — bypassing it —
would violate the objective's "without bypassing gates" clause.
