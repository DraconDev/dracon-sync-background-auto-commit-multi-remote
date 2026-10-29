# Read-only fleet status query timed out — 2026-10-29

At approximately 00:45 UTC, the read-only `dracon-sync repos` query produced no
output and exceeded its 30-second command limit. No matching `dracon-sync repos`
process remained after the timeout. This attempt gives no fleet-state result and
must not be counted as a successful observation or as evidence that the daemon
is unhealthy.

Immediately before the query, the host reported load averages 97.45 / 91.60 /
82.95, 28 GiB memory used with 1.6 GiB available, and 34 GiB swap in use. This
is contextual evidence only; no causal attribution for the CLI timeout is
established.

The `dracon-sync` checkout itself was clean and equal to its local
`origin/main` tracking ref (0 ahead / 0 behind), but this is only repository-local
state, not a fresh advertised-ref check and not a full-fleet result. The previous
35/36 structural assessment remains historical; a fresh all-repository clean-idle
observation is still required.

**Next:** retry the read-only fleet query after host resource headroom improves,
then record every watched repository and any inspection failures without
reducing scope.
