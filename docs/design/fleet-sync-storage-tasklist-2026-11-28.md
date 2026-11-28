# Fleet sync and storage tasklist

## Publication

- [x] Give short pushes a path past a long-running hook when host memory allows. Verified `.dracon` publishing while the browser hook was running.
- [x] Show configured GitHub owners in distinct colors and use 2 GiB for the size review and automatic transfer to `dracon-uk`.
- [x] Restore the two TabMon browser test suites in the publication sandbox. The suites passed in the live gate and GitHub received the fix.
- [ ] Verify the fleet reaches zero outstanding work after producers settle. Track publication admission, hook execution, and memory holds when a repo stays active for several minutes.
  - A live check matched GitHub `main` to local `HEAD` for writer, browser, platform, and `.dracon`. A subsequent inventory showed 38 clean, three active, and no pending items, warnings, concerns, or inspection failures. Continue checking for sustained queues and freezes.

## Local storage

- [x] Compact writer, browser, and Hellhunter while preserving branch tips. Keep the private recovery archives.
- [x] Move background spool maintenance off the commit path. The latest targeted sweep removed 54 writer, 232 platform, and 114 browser spools after proving their data was already published.
- [ ] Qualify one writer spool for recovery and measure disk usage before and after, including recovery-store growth. Writer remains the largest source of aged spool bytes. Follow the [durable recovery requirements](epoch-spool-durable-recovery-2026-10-31.md).
  - Stopped the full-closure trial after it had written 3.6 GiB for a 35 MiB source spool. The original directory and index hash were verified after the attempt.
  - A scratch repository using hardlinked private archive objects and a 19.8 MiB supplemental pack reconstructed the sampled index tree and verified both producer endpoints, including 988,623 required objects. Its exact metadata copy took 34.8 MiB. The first sample therefore adds about 20 MiB even if its source could be removed. Measure incremental cost across multiple spools before qualifying deletion.
- [ ] Repeat the fleet health check after recovery. Preserve spools whose ownership or contents cannot be proved.

## Git LFS

- [x] Audit all 41 repository tips and recent asset intake. `ai-auto-video` is the first candidate for existing large media. The scratch conversion verified 11 pointers and their restored hashes across 519.5 MiB of media.
- [x] Make Git LFS available on the daemon's existing PATH. Local tests produced canonical pointers and restored original bytes for generated writer EPUBs and covers and for browser design evidence PNGs.
- [ ] Publish the verified video files through the existing GitHub LFS endpoint after approval of the exact files, then confirm retrieval from a fresh cache.
- [ ] Prepare forward-only LFS intake for writer's generated book EPUBs and covers. The seven-day audit found about 2.1 GiB of ordinary EPUB and JPEG blob churn.
- [ ] Prepare browser design evidence PNGs for forward-only LFS intake after writer. The seven-day audit found about 733 MiB of ordinary PNG blob churn, with Adblock Pro design evidence the most frequently changed group.
