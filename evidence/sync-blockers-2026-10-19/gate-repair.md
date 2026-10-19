# Workspace gate repair (bounded, no cleanup)

The first two workspace test runs failed dracon-system's real-process
cleanup-safety test because PID 1029602 was a zombie bash with empty argv.
The package-cache scanner treated empty argv as an unreadable LIVE wrapper,
refusing before it reached the test's live-node fixture. /proc status showed
State Z and PPid 1029574; no process was killed, reaped or otherwise changed.

Corrected only the empty-wrapper classification: skip an empty-cmdline
wrapper IF readable proc status affirmatively says Z (zombie). Missing,
malformed or sleeping status still refuses. This preserves the cleanup
safety invariant: a zombie cannot execute or hold a build/cache open.
Regression tests exercise all those states. The existing real live-process
safety test now passes and workspace gates pass without excluding it.

Changes are in dracon-system source/tests; this goal deploys ONLY the
qualified dracon-sync binary, not a new guard binary/service.
