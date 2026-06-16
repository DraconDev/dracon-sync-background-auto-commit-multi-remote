# Dracon Sync

Background, auto-commit, multi-remote — invisible git sync for developer workspaces.

This repository is a feature façade for `dracon-sync`. It does **not**
duplicate the implementation code. The canonical source of truth remains the
[`DraconDev/dracon-utilities`](https://github.com/DraconDev/dracon-utilities)
monorepo, with this utility's code and docs under:

- Source: [`dracon-sync/`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync)
- User guide: [`dracon-sync/README.md`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync/README.md)
- Design notes: [`dracon-sync/BLUEPRINT.md`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync/BLUEPRINT.md)
- Example config: [`dracon-sync/dracon-sync.example.toml`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync/dracon-sync.example.toml)

## Why this name?

The descriptive name is a deliberate choice for Codeberg/Forgejo, where
descriptive repo names get upvotes and free attention because readers
immediately know what the project does. The full word list (no fillers, no
audience/UX claims) is documented in
[`docs/design/github-feature-repos.md`](https://github.com/DraconDev/dracon-utilities/blob/main/docs/design/github-feature-repos.md).

## Purpose

Watches configured repositories, waits for changes to settle (fingerprint stability / debounce), commits deterministic diff-based messages, and pushes to origin plus configured mirrors. Invisible: runs in the background, no user interaction required.

Use this repo to feature the utility on GitHub, GitLab, and Codeberg without
splitting the actual implementation out of the monorepo. Issues, project
boards, and roadmap notes can live here, while commits, releases, tests, and
packaging stay anchored in `dracon-utilities`.

## Runtime

- Binary: `dracon-sync`
- Service: dracon-sync.service
- Example policy: `dracon-sync/dracon-sync.example.toml`
- Common commands: `dracon-sync status · dracon-sync repos · dracon-sync health · dracon-sync daemon`

## Relationship to the monorepo

| Boundary | Decision |
|----------|----------|
| Source code | Lives in `dracon-utilities/dracon-sync` |
| Release artifacts | Built and published from `dracon-utilities` |
| Feature surface | This façade repo (and short-name alias) |
| Operational policy | `~/.dracon/utilities/` TOML files |
| Shared libraries | Sibling `dracon-libs` workspace where applicable |

## Maintenance

When the monorepo changes the utility README, blueprint, or example config,
regenerate this façade with:

```bash
cd /path/to/dracon-utilities
./scripts/scaffold_feature_repos.py --apply --repo dracon-sync
./scripts/scaffold_feature_repos.py --push-all-remotes --repo dracon-sync \
    --ssh-target /path/to/dracon-sync-background-auto-commit-multi-remote
```

Do not paste implementation code into this façade repo. Keep it as a stable
navigation and feature surface so the monorepo remains the single source of
truth.

## License

AGPL-3.0-only — see [LICENSE](LICENSE).
