# Fleet convergence after the youtube-filter harness repair

## Observation (daemon-reported, `dracon-sync repos --json`)

    before containment lift : ok=16 active=5 warn=15 concern=0 failures=0
    after  containment lift : ok=30 active=5 warn=1  concern=0 failures=0
    after gate repair       : ok=34 active=2 warn=0  concern=0 failures=0

`fleet-converged.json` is that final observation. Zero repositories are AHEAD,
zero are PENDING, zero warnings, zero concerns, zero failures.

`browser-extensions-shared` — the repository that blocked everything — reached
`ahead = 0` with local == origin == gitlab at `b7ca4a1b0f`, after being 1609
ahead with no push for two days:

    local  : b7ca4a1b0f89b283f9cee0c9ee437ce4a5e71bad
    origin : b7ca4a1b0f89b283f9cee0c9ee437ce4a5e71bad
    gitlab : b7ca4a1b0f89b283f9cee0c9ee437ce4a5e71bad

## Destination verification, and an honest note about how it was measured

A naive sweep comparing local HEAD to each remote tip reported 4 mismatches, all
on the gitlab leg. That was a MEASUREMENT artifact, not a publication failure:
these repositories commit roughly once a minute, so HEAD moves between the
`rev-parse` and the `ls-remote`. Re-checking each candidate showed
`gitlab tip is an ancestor of local HEAD, 0 commits behind` for all four
(`.dracon`, `browser-extensions-shared`, `ai-auto-writer`, `DraconDev`) within
seconds.

The reliable statement is therefore not "every tip matched at one instant" but:

- 36 watched repositories, all with remotes;
- 32 matched every intended remote at the sampled instant;
- the other 4 were behind by AT MOST the commits their own agents made during
  the probe window, with gitlab an ancestor in every case and no divergence;
- zero repositories were ahead of local, so nothing is stranded.

## The three DIRTY repositories are live editing, not convergence debt

`dracon-platform`, `dracon-strategy/ai-auto-writer` and
`dracon-platform/web/games/wip/junk-runner` are DIRTY with **push = OK** — their
committed work is published; only in-flight working-tree edits remain (nested
submodule pointers, book .epub/.json outputs, `.pi-glla` audit state). The
objective explicitly allows continued editing to delay the idle observation, and
requires not stopping producers to manufacture it. Per the daemon's own
3-second debounce plus commit/push time, DIRTY is the expected steady state for
a repository under active editing, not evidence of a stall.
