# GitHub Feature Façade Repositories

Dracon utility façade repositories are intentionally small GitHub presentation
surfaces. They make `dracon-sync`, `dracon-system`, and `dracon-warden` easier to
feature on GitHub without splitting the implementation out of the
`DraconDev/dracon-utilities` monorepo.

## Invariants

1. The monorepo is the only source of truth for implementation code, tests,
   release packaging, and changelog entries.
2. Façade repos contain only navigation, issue/project metadata, licenses, and
   links back to the monorepo paths.
3. Do not copy implementation files into façade repos. If code needs a public
   home, create a real separate crate/binary repo and update the monorepo
   architecture docs first.
4. Regenerate façade repos with `scripts/scaffold_feature_repos.py --apply` so
   the presentation layer stays consistent.

## Why this is not a hack

GitHub cannot natively present a subdirectory as a first-class repository with
separate issues, projects, topics, and README without duplicating or moving
files. A façade repo avoids both bad options:

- Moving code would split the implementation and break the current release
  pipeline.
- Copying code would create drift and duplicate maintenance.

The façade repo is therefore a documented, scripted boundary: it owns GitHub
feature metadata only, while `dracon-utilities` owns code and releases.
