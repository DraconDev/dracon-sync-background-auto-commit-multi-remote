## Review

- **Fixed: Previous P1 resolved.** Tag pushes now enter `push_phase` (`src/release.rs:72–82`); janitor deletions do likewise (`src/git/mod.rs:5188–5195`). Local tag creation, rollback, and janitor mutations remain outside the released-permit interval.
- **Correct:** Nix PR pushing is also enrolled (`src/nix.rs:229–236`). Preparation remains outside the phase, and normal success/error completion reacquires local capacity before restoring the previous branch (`src/nix.rs:238–241`). The production caller awaits this directly, preserving task-local scope (`src/nix.rs:150`).
- **Correct:** The extended real-hook fixture exercises all three production routes with local concurrency one. It verifies hook nice 19, unrelated real inspection and commit before hook release, final remote results, returned capacity, and Nix branch restoration (`src/sync/push_isolation_tests.rs:179–291`).
- **Correct:** These additions preserve existing command arguments, hook enforcement, timeout handling, and scoped rewrite exception. Outside daemon enrollment, `push_phase` remains a passthrough (`src/git/push_resources.rs:46–57`).
- No remaining bypass relevant to the previous finding was identified in the reviewed source paths.

No issues found.

**Validation:** Independent review was source/artifact inspection only. Parent-produced `late-entry-final.log` records the extended test passing; the parent reports exit 0. Full post-Nix workspace/Clippy/release reruns remain pending per the handoff; earlier gates are not claimed as final-Nix validation.

**Merge verdict: OK with notes** — previous source blockers are resolved; complete the planned final validation before deployment.

## BLOCKERS

None found in this narrow follow-up.