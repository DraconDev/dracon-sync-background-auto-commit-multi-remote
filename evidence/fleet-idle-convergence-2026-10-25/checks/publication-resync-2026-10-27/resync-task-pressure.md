# Resync observation — 2026-10-27

The authoritative goal is autonomous convergence, not the unrelated bucket-strategy handoff excerpt. Full-fleet JSON captured at `/tmp/fleet-resync.json`: 36 repositories, 30 OK, 5 active, 1 warn; acceptance remains unmet.

Targeted browser checks: Tabmon user-flows 44 passed on the checkout observed before the test (HEAD f9a5e02f). Affiliate CI passed with real origin push tuples supplied on stdin. Earlier empty-stdin CI failed its intentional build-everything missing-source guard; this was not a faithful hook invocation. The receipt `browser-targeted-gates-f9a5e02f.json` captured HEAD after these live-checkout checks, not an immutable tested revision: its recorded 92349e3a must NOT be interpreted as a frozen original-hook pass.

Browser retry was re-enabled once after targeted checks passed. Actual daemon original-hook attempt on 92349e3a failed, not passed: `/home/dracon/.local/state/dracon/sync/gates/browser-extensions-shared/dracon-pre-push.q9MEkY/manifest.json`, dependency installation exit 128. Raw install log reports bookmarks-new-tab-page WXT postinstall SIGABRT. No new retry reset was issued after this failure.

Resource observation: system service TasksCurrent 496 / TasksMax 512, then cgroup pids.current 489 and pids.events max 138. This supports investigating service-wide task exhaustion as a shared integration failure; it does not by itself prove which allocation caused the WXT abort. Native pool caps were already present (Tokio/Rayon/Rolldown/GOMAXPROCS), provisioning was serialized and lease released. Preserve finite task bounds and independent publication; do not bypass lifecycle scripts or gates.

Daemon publication continues: observed git push gitlab for `.dracon` snapshot 98918e70 (17+ minutes) and browser snapshot 92349e3a (5+ minutes). In-flight commands are not successful-publication proof. No manual push was performed. Remaining work includes original gates, canonical-writer preservation qualification, coherent workspace gates, all intended remote proofs and entirely clean/idle fleet.
