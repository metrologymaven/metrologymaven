---
id: D-042
title: Generated reports are refreshed on main, not in every PR
status: accepted
raised_by: maintainer (build throughput)
blocks: []
decide_by: 2026-11-18
updated: 2026-11-18
---

# D-042: Generated reports are refreshed on main, not in every PR

**Status: ACCEPTED 2026-11-18, maintainer.**

## Question

Must every pull request commit freshly regenerated files under `generated/`
(`BUILT.md`, `INDEX.md`, `COVERAGE.md`, `DECISIONS.md`, `EXPERT-QUEUE.md`,
`milestones/*.md`)?

## Context

- Every leaf PR changes a `NODE.md` (`built_in`, `built_pr`), so every PR also changed the
  same generated report lines. Each merge therefore put every other open PR into conflict
  or made its `spec-check` job fail as stale, and each needed a refresh and a full CI
  re-run (20–40 minutes) before it could merge.
- The reports are derived data: `python tools/scope.py check` rebuilds them exactly from
  the spec tree, so committing them in a PR adds no information a reviewer needs.

## Decision

1. Pull requests do not need to commit regenerated `generated/` files. The `spec-check`
   job still runs `scope.py check` and the milestone report on every PR, so an invalid
   spec tree or coverage error still fails the PR.
2. The "generated reports are up to date" step stays in `spec-check`, but is
   informational (`continue-on-error`), on PRs and on main.
3. A maintainer refreshes `generated/` on main with a small regeneration PR after a batch
   of merges (at least daily while PRs are merging). Anything that reads the reports
   (the expert-queue sync, the site) may lag main by at most that batch.
4. Merge tooling resolves any `generated/` conflict by taking main's version.

## Consequences

- PRs no longer conflict with each other on `generated/`, so approved PRs can merge back
  to back without a refresh cycle.
- Between regeneration PRs, the files under `generated/` on main can be behind the spec
  tree. The spec tree (`spec/**/NODE.md`) is the source of truth.
- No required check is removed or weakened: `check` is still required and still fails on
  spec errors.

Also in the same change: every workflow run on a pull request is cancelled when a newer
commit is pushed to the same PR (`concurrency` with `cancel-in-progress` for
`pull_request` events only). Runs on main are never cancelled.
