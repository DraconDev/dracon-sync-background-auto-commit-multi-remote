## Review

Paths below are relative to `extensions/standalones/tabmon/`.

- **Correct:** Historical provenance separation preserves meaningful guards. `tests/sfx.test.ts:834-859` retains exact prompt/model/path assertions; `:1266-1331` still measures shipped tempo and rejects the 1.25× sped-up control. Desired `Moment.bpm` values remain checked against desired recipes (`tools/test_music_provenance.py:143-152`).
- **Correct:** Plans cannot overwrite generation manifests; successful records bind requests and source hashes. Legacy claim changes are rejected, measurement refreshes preserve existing facts, and receipt writes use locking plus expected-byte comparison (`tools/music_provenance.py:42-102`).
- **Correct:** Removing `THEME_REGISTERS` leaves the existing label order intact (`lib/game/themes.ts:53-58`). The dead-export gate remains unchanged.
- **Fixed:** None; read-only review.

### Findings

- **P1 — Catalog parsing silently fails for current themes.**  
  `tools/merge-music-receipt.py:43-48` requires `id` immediately followed by `name`, but current entries insert `register` between them (`lib/game/themes.ts:71-75`). Consequently, `catalog()` returns no entries, and newly generated theme merges silently omit `title` and `shipped_mood` (`tools/merge-music-receipt.py:120-123`).  
  **Smallest fix:** Accept the register field and require a catalog match for merged themes. Test the real catalog; the integration test currently mocks it away.

- **P1 — Recipe revision changes do not stop the active fetch batch.**  
  `tools/fetch-sfx-mmx.py:308-309` checks the revision only before generation. Its loop (`:331-354`) continues making requests from cached prompts after the recipe changes. `tools/fetch-missing.py:126-136` detects that change only after the entire child batch exits.  
  **Smallest fix:** Recheck the pinned digest immediately before each request, after pacing/acquiring the lease; stop without discarding completed successes. Add an offline two-take test that changes the recipe after take one and asserts take two never runs.

- **P2 — Current documentation still asserts unshipped tempo changes.**  
  `docs/sfx-sources.md:109-114` says shipped themes span 64–112 BPM, contradicting `docs/sfx-shipped-mastering.json:5` and the unchanged historical audio contract.  
  **Smallest fix:** Describe 64–112 BPM as desired future targets; identify shipped mastering as 100 BPM. This appears to be concurrent documentation work, not necessarily this executor’s change.

### Verification limits

Reviewed current source and the supplied diff artifact; no commands, API calls, or edits performed. Historical commit recovery was not independently verified. No executable test tool was available; supervisor should run:

`python3 -m unittest discover -s tools -p test_music_provenance.py`

**Merge verdict: BLOCK** pending the two P1 fixes.

BLOCKERS: Catalog metadata omission; active batches continue requesting takes after recipe revision changes.