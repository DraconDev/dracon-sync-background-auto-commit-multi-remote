# dracon-sync remote-scope gap — 2026-10-29

The current dracon-sync checkout at `406be7273a57` directly advertises from its
configured GitHub (`origin`) and GitLab remotes. Both `main` refs match the local
HEAD. The repository's configured remote list contains only those two remotes;
there is no Codeberg alias or push URL.

The parent `AGENTS.md` documents Codeberg as an intended dracon-sync
publication destination. A read-only query to the documented Codeberg repository
found `main` at `18dba43b3260`, dated 2026-08-11, which is an ancestor of local
HEAD with 4,943 commits between the two. Therefore GitHub/GitLab publication is
not proof of all-destination convergence.

The root design `docs/design/codeberg-quota-leak-fix-2026-07-13.md` records
Codeberg at its 85 GiB grace quota and explicitly defers historical cleanup;
the active goal prohibits history rewrites/force-pushes without authorized
operator maintenance. No remote was added and no fetch, push, rewrite, or quota
cleanup was attempted. This is a real publication-scope gap, not a reason to
label the repository healthy or reduce the required destination set.

**Next:** the operator must confirm whether Codeberg remains an intended
publication destination and authorize a quota-safe, policy-compliant plan before
any reconnect or publication. The evidence is also machine-readable in
`dracon-sync-remote-scope-2026-10-29.json`.
