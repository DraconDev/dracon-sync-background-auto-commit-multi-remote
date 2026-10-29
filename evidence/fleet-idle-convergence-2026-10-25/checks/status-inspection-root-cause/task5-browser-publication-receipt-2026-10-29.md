# Browser publication receipt — 2026-10-29 01:12 UTC

The long GitLab pre-push gate for commit `5f35a18ed8fb` finished with status
`passed` at 01:10:33. The corresponding `git push gitlab` process was observed
as a direct child of `dracon-sync-operator` and was absent by the 01:12:44
verification. At that time, read-only `ls-remote` queries showed both distinct
configured push destinations advertising `5f35a18ed8fb` on `refs/heads/main`:
GitHub (the `github` and `origin` names share one URL) and GitLab.

The local browser repository had already advanced to `3e504bba9dc7`, with the
published commit confirmed as its ancestor and 70 commits beyond it. The daemon
later reported 73 unpushed commits at 01:13:50 and continued to say publication
was owned by another worker; no matching browser push process appeared in the
01:13:51 process snapshot. Thus one publication epoch reached the advertised
refs, but the current browser tip is not proven published and the fleet is not
idle. No manual push was performed.

The detailed machine-readable receipt is adjacent. The gate manifest remains at
`/home/dracon/.local/state/dracon/sync/gates/browser-extensions-shared/dracon-pre-push.yZFugk/manifest.json`.
