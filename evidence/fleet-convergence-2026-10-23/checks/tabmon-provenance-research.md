# Code Context

## Files Retrieved
All relative paths below are under `/home/dracon/Dev/browser-extensions-shared/extensions/standalones/tabmon/`.
1. `docs/sfx-sources.md` (lines 1-260, 297-331, 450-520) — desired orchestral recipes, contradictory shipped claims, unsafe refresh instructions.
2. `docs/sfx-manifest-music-3.0.json` (lines 1-60; all rows compared programmatically) — committed historical chiptune generation provenance.
3. `tools/fetch-sfx-mmx.py` (lines 99-321) — shared parser, actual request construction, generation, manifest-only hazard.
4. `tools/merge-music-receipt.py` (lines 1-134) — blindly replaces generation fields from supplied manifest while adding measurements.
5. `tools/measure-set.py` (lines 1-130) — measures shipped bytes; cannot establish generation prompt or model.
6. `tests/sfx.test.ts` (lines 420-453, 819-905) — exact recipe/receipt equality, shipped paths, staged-batch checks, tempo target coupling.
7. `tools/fetch-missing.py` (lines 1-134) — retries by source existence/size, not recipe identity.
8. `tools/package-sfx.py` (lines 125-165, 208-240) — desired variable tempi already replaced shipped-set targets.
9. `NOTE.md` (lines 1820-1868) — original receipt rationale and explicit precedent for committing desired recipes before generation.
10. `docs/theme-batch-2026-10-23.json` (lines 1-55) — shipped batch still explicitly chiptune/100 BPM.
11. `.pi-glla/owner.json`, `.pi-glla/session-owner.json` (line 1 each) — recorded owner quit 2026-10-23T22:39:24Z; local goals directory empty.

## Key Code
- `parse_prompts()` selects every `## The prompts...` section, keyed only by moment; duplicate ids overwrite earlier rows (`tools/fetch-sfx-mmx.py:112-146`). Archiving and pending recipes cannot safely share that namespace.
- Receipt test requires `row.prompt === prompts.get(name)?.prompt`, model `music-3.0`, and actual shipped path existence (`tests/sfx.test.ts:819-858`). This guard is meaningful, not an external blocker.
- `--manifest-only` serializes CURRENT recipes without generating (`tools/fetch-sfx-mmx.py:283-292`). Normal fetch also writes every requested recipe after failures, not only successfully generated takes (`294-321`). Neither output alone proves a take was made.
- Merge starts `row = dict(source[key])` and assigns `receipt['takes'][key] = row` (`tools/merge-music-receipt.py:93-126`); filesystem size and measurement additions do not authenticate that source's prompt.

## Architecture
Desired markdown recipe -> fetch parser -> API request -> raw take -> packager -> shipped MP3.
Separately, fetch manifest + measurement map + catalog -> committed receipt. Tests currently conflate the mutable desired recipe with the generation recipe for shipped bytes.
`build_body()` appends key, instruments/mood and use-case (`fetch-sfx-mmx.py:183-212`); receipt `prompt` is the base table prompt, not the complete API request. Preserve this historical schema honestly; do not retroactively claim it stores exact wire payloads.

## History and current activity evidence
- Read-only Python comparison: current parsed recipes = 37; receipt takes = 37; prompt differences = 37.
- Same parser on `git show 916b7be2f3^:extensions/standalones/tabmon/docs/sfx-sources.md`: 37 historical rows, **37/37 exact receipt prompt matches**. Historical recipe recovery is demonstrated, not guessed.
- `916b7be2f3` (Oct 24 02:54) changed capture-hit and moonlit-grove to orchestral/folk recipes. `a0b865b194` (03:13) changed remaining prompts, parameters and even replaced 13 measured mix rows with recipe text. This was documentation churn, not an audio commit.
- Latest manifest commit: `2ff653411d`, Oct 23 21:57. Latest shipped-media commit observed: `18f8a3d6c0`, Oct 23 21:56. All recipe re-voice changes came later.
- `0c765e7325`, Oct 24 03:18, changed packager pulse targets to 64-112 BPM. Current tests derive shipped-file expectations from that mutable table (`tests/sfx.test.ts:426-434`): splitting prompts alone may expose/leave another genuine product failure.
- `20c69a2b94`, Oct 24 04:39, added prose claiming the orchestral set already shipped and discussing allowance exhaustion after 17 takes. That prose appeared during inspection; it is not evidence of corresponding orchestral audio or successful generation.
- No matching fetch-missing/fetch-sfx/package/merge/measure processes were observed. No orchestral source directory or active generation log was established in bounded `/tmp` inspection. This does NOT prove no generation exists elsewhere.
- Existing batch JSON says shipped chiptune; NOTE documents older chiptune receipts and historical pending-batch/quota precedent, not a completed Oct 24 re-voice. Local goal directory has no files; owner state records quit. No authoritative new orchestral-generation authorization was found.
- Working tree changed concurrently: sfx-sources.md briefly dirty, then unrelated auto-form-filler files changed. No staged paths were observed. Preserve all work; re-read before any correction.

## Start Here
Open `docs/sfx-sources.md` alongside the historical version at `916b7be2f3^`. First separate shipped facts from future intent; do not modify receipt prompts or regenerate media.

## Recommended narrow durable correction
1. Keep every orchestral recipe/parameter and rationale intact in an explicitly pending, separately versioned recipe document/section excluded from the shipped-recipe parser. Label re-voice as desired/pending, not completed; preserve the request's intent, not the unsupported completion claim.
2. Restore an explicitly immutable shipped-generation recipe from the demonstrated pre-revoice history, retaining exact model/base-prompt equality to the unchanged historical receipt. Keep equality/model/path gates fully operative; do not replace the comparison with mere existence or waive it.
3. Give pending generation its own explicit recipe input/revision selection in fetch and fetch-missing. Avoid duplicate-id precedence or silent default recipe switching. Cover pending parsing independently, with shipped provenance still checked exactly against its historical recipe.
4. Separate desired packaging targets from shipped mastering targets too. Preserve variable-tempo intent as pending configuration; do not let it rewrite the expected pulse of unchanged shipped audio. Confirm actual tempo failures with the real product suite before declaring repair complete.
5. Remove the documented recipe-dump-to-receipt refresh workflow (`sfx-sources.md:487-500`). Measurement-only refresh must preserve generation fields from existing receipts/authentic source manifests; merge must reject a recipe-only plan as proof of generation. Future successful fetches should record per-success provenance tied to raw bytes/request and avoid stamping failed or stale takes.
6. This is a product provenance/workflow repair, not a media feature task. Keep shipped MP3s and historical receipt facts unchanged. Add regression coverage only in a separately authorized implementation pass; this investigation changed no code/assertions.

## Authorization and residual risks
No external blocker is demonstrated. Local provenance/schema/tempo coupling is repairable in scope without MiniMax access, new takes, or billing changes. Actual orchestral generation, quota spending/top-up, replacing shipped audio, and corresponding audio-derived labeling need separate authorization/authentic generation evidence. Do not infer permission from newly written completion prose. Concurrency is a real coordination risk; agree ownership before editing the live seam.

```acceptance-report
{"criteriaSatisfied":[{"id":"criterion-1","status":"satisfied","evidence":"Read-only history/parser comparison demonstrated 37/37 historical matches and current mismatches; identified safe split and authorization boundary."}],"changedFiles":[],"testsAddedOrUpdated":[],"commandsRun":[{"command":"Read-only git status/log/show/diff, targeted source reads, process inspection, python3 -B parser comparison","result":"passed","summary":"37 current recipe/receipt mismatches; all 37 match pre-revoice history; no staged paths or matching generation process observed."},{"command":"Full product tests","result":"not-run","summary":"Investigation only; no assertions changed or media generated."}],"validationOutput":["current rows 37 receipt 37 prompt differences 37","pre-revoice rows 37 historical exact prompt matches 37"],"residualRisks":["Concurrent seam edits; bounded inspection cannot exclude generation elsewhere.","Variable-tempo packaging expectations also drifted from unchanged shipped media."],"noStagedFiles":true,"diffSummary":"No repository edits; report artifact only.","reviewFindings":["Local blocker: mutable desired recipes are incorrectly treated as immutable generation provenance.","Workflow hazard: manifest-only recipe dump can overwrite historical generation facts through merge."],"manualNotes":"No media generation, receipt fabrication, code edits, staging, commit, push, or child agents. Some shell discovery attempts failed harmlessly due wrapper/path behavior; bounded evidence used successful scoped inspection."}
```
BLOCKERS:
