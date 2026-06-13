# Dracon Sync

Invisible git sync automation for AI-assisted development.

This repository is a GitHub feature façade for dracon-sync. It does **not**
duplicate the implementation code. The canonical source of truth remains the
[`DraconDev/dracon-utilities`](https://github.com/DraconDev/dracon-utilities)
monorepo, with this utility's code and docs under:

- Source: [`dracon-sync/`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync)
- User guide: [`dracon-sync/README.md`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync/README.md)
- Design notes: [`dracon-sync/BLUEPRINT.md`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync/BLUEPRINT.md)
- Example config: [`dracon-sync/dracon-sync.example.toml`](https://github.com/DraconDev/dracon-utilities/tree/main/dracon-sync/dracon-sync.example.toml)

## Purpose

Watches configured repositories, waits for changes to settle, commits deterministic diff-based messages, and pushes to origin plus configured mirrors.

Use this repo to feature the utility on GitHub without splitting the actual
implementation out of the monorepo. Issues, project boards, and roadmap notes can
live here, while commits, releases, tests, and packaging stay anchored in
`dracon-utilities`.

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
| GitHub feature surface | This façade repo |
| Operational policy | `~/.dracon/utilities/` TOML files |
| Shared libraries | Sibling `dracon-libs` workspace where applicable |

## Maintenance

When the monorepo changes the utility README, blueprint, or example config,
regenerate this façade with:

```bash
cd /path/to/dracon-utilities
./scripts/scaffold_feature_repos.py --apply --repo dracon-sync
```

Do not paste implementation code into this façade repo. Keep it as a stable
navigation and feature surface so the monorepo remains the single source of
truth.

## License

AGPL-3.0-only — see [LICENSE](LICENSE).
