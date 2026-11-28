# Fleet sync and storage tasklist

## Publication

- [x] Give short pushes a path past a long-running hook when host memory allows. Verified `.dracon` publishing while the browser hook was running.
- [x] Show configured GitHub owners in distinct colors and use 2 GiB for the size review and automatic transfer to `dracon-uk`.
- [x] Restore the two TabMon browser test suites in the publication sandbox. The suites passed in the live gate and GitHub received the fix.
- [ ] Verify the fleet reaches zero outstanding work after producers settle. Track publication admission, hook execution, and memory holds when a repo stays active for several minutes.

## Local storage

- [x] Compact writer, browser, and Hellhunter while preserving branch tips. Keep the private recovery archives.
- [x] Move background spool maintenance off the commit path. The latest targeted sweep removed 54 writer, 232 platform, and 114 browser spools after proving their data was already published.
- [ ] Qualify one writer spool for recovery and measure disk usage before and after, including recovery-store growth. Writer remains the largest source of aged spool bytes. Follow the [durable recovery requirements](epoch-spool-durable-recovery-2026-10-31.md).
- [ ] Repeat the fleet health check after recovery. Preserve spools whose ownership or contents cannot be proved.

## Git LFS

- [x] Audit all 41 repository tips and recent asset intake. `ai-auto-video` is the first candidate for existing large media. The scratch conversion verified 11 pointers and their restored hashes across 519.5 MiB of media.
- [ ] Publish the verified video files through the existing GitHub LFS endpoint after approval of the exact files, then confirm retrieval from a fresh cache.
- [ ] Review writer EPUB and cover churn, then browser PNG churn. Use the producer paths and growth rate to select assets for LFS. Keep shared history rewrites subject to separate approval.
