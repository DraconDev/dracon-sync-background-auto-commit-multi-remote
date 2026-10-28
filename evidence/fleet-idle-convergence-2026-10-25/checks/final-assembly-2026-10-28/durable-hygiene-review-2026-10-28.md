## Review

- **Fixed: P1, symlink retirement — closed.** `dracon-sync/src/sync/hygiene_intent.rs:427–447` now validates one exact `.gitignore` entry, regular mode `100644/100755`, blob type and valid OID before reading content. Symlink target text cannot authorize retirement. Regression checks pending intent, unchanged symlink inode/target, artifact bytes and index (`hygiene_tests.rs:81–125`).

- **Fixed: P1, whitespace-equivalent retirement — closed.** `dracon-sync/src/exclude/hygiene.rs:20–36` preserves requested whitespace and compares existing lines exactly. Leading-whitespace literal names are escaped (`:109–122`). Native-Git tests verify space/tab/NBSP names without broadening matches (`:181–226`). The retirement regression proves `" dist/"` does not suffice, canonical correction remains pending until committed, and the corrected rule actually ignores the probe (`hygiene_tests.rs:129–222`).

**No issues found.** This conclusion covers the two corrections, not broader preservation qualification.

### Secondary assessment: rollback remains unqualified

`dracon-sync/src/sync/excluded_revert.rs:798–802` exchanges the inodes, then exchanges back on fingerprint mismatch or lease break. Leases cover the original inode (`:743`), not the newly canonical desired inode.

Consequently, **if another producer obtains a writable FD to the desired inode between those exchanges, rollback relocates that producer’s inode into the private location without retargeting its FD**. This follows from the exchange sequence; whether the proposed queued-old-opener schedule reliably reaches that branch was not demonstrated here.

I cannot establish a Linux guarantee that blocked lease-open handling restarts pathname lookup in a way that closes this window. Even such restarting would not itself protect a separately completed open of the desired inode. Existing modeled displaced-FD tests explicitly do not establish the actual queued-open syscall interleaving (`excluded_revert_recovery_tests.rs:350–352`).

**Required qualification:** deterministically pause around final lease check, first exchange and rollback; queue the old-inode open, complete a new canonical writable open, then record both FD inode identities and write destinations through rollback/settlement. Until then, retain this as an explicitly open preservation question—not a reproduced new finding or a safety guarantee.

### Limits and verdict

- Source re-read at nested `main` ref `4be938222a65a9ce04b77add080586eebdc31d55`; no committed-range diff supplied.
- Supplied green log records 24 passing targeted tests. No commands, builds or tests were run by this reviewer.
- Historical v2 four-gate qualification does not qualify the post-fix assembly. No deployment, loaded-runtime or full-fleet approval.
- **Merge verdict: OK with notes for these two corrections only.** Broader queued-open/exact-exchange qualification remains open.