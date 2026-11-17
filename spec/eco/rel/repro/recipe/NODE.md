---
id: ECO.REL.REPRO.RECIPE
title: Third-party offline rebuild recipe
level: 4
parent: ECO.REL.REPRO
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.REPRO-004]
covers: []
depends_on: [ECO.REL.REPRO.PIN, ECO.REL.REPRO.STRIP]
side_effect: W1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.REPRO.RECIPE: Third-party offline rebuild recipe

## Purpose

Lets anyone outside the project rebuild a release and compare it with the published artefacts: a written recipe (`REBUILD.md`) and one command that fetches every pinned input once, then builds offline and runs the comparison. A CI job follows the recipe on a clean machine to keep it working.

## Scope

**In:**
- `REBUILD.md`: prerequisites, the fetch step, the offline build step and the compare step
- `cargo xtask rel rebuild --tag <t> --fetch` (online, fetches source archive, vendored dependencies and builder image by digest into a cache folder) and `--offline` (builds from that folder with network disabled)
- A recipe-follow CI job on a fresh runner

**Out:**
- The project's own second build (owned by `ECO.REL.REPRO.REBUILD`)
- Pin definitions and comparison rules (owned by `ECO.REL.REPRO.PIN` and `ECO.REL.REPRO.STRIP`)

## Requirements

- **REQ-ECO.REL.REPRO.RECIPE-001** The fetch step shall download only inputs whose digests are recorded in `build-env.lock` or the tag's source, and shall fail if any downloaded digest differs.
- **REQ-ECO.REL.REPRO.RECIPE-002** After the fetch step, the build and compare steps shall complete with all network access disabled.
- **REQ-ECO.REL.REPRO.RECIPE-003** The compare step shall print, per artefact, the status that `ECO.REL.REPRO.STRIP` defines, and the published SHA-256 it compared against.
- **REQ-ECO.REL.REPRO.RECIPE-004** A CI job shall follow `REBUILD.md` verbatim for the `fid` CLI and `fid-verify` on Linux after every release and fail if the result is not `identical` or `identical_after_strip`.

## Acceptance criteria

- **AC-1** Given the fixture tag in `fixtures/rel/recipe/` (a hello-world workspace and its lock), when `cargo xtask rel rebuild --tag v0.0.1-fixture --fetch --cache c/` then `--offline --cache c/` run, the second with the network disabled, then the compare output reports `identical` for the fixture artefact.
- **AC-2** (error path) Given a tampered cached vendored archive, when `--offline` runs, then it fails with `RECIPE-DIGEST` naming the input.
- **AC-3** Given the CI recipe-follow job, when it extracts the blocks tagged `rebuild` from `REBUILD.md` and runs them on a fresh Linux runner, then it exits 0 for the fixture release.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel rebuild --tag <t> (--fetch | --offline) --cache <dir> [--compare-with <url-or-dir>]` (W1, writes into the cache and output folders).
- **Data structures:** cache layout `<cache>/<digest>` content-addressed.
- **Algorithm:** fetch by digest; verify; build in the pinned container with network disabled; compare using the STRIP tool.
- **Errors:** `RECIPE-DIGEST(input)`, `RECIPE-NETWORK-REQUIRED` (if any build step tries the network).
- **Determinism:** N (container runtime); outputs are compared, not assumed.
- **Performance budget:** n/a.
- **Crate:** `fid-xtask` (module `rel::recipe`); `REBUILD.md` at the repository root.

## Test fixtures

`fixtures/rel/recipe/` with a hello-world workspace, lock file and expected digest; CC-BY-4.0.

## Build prompt

> Write `REBUILD.md` and add `cargo xtask rel rebuild --tag <t> (--fetch | --offline) --cache <dir> [--compare-with <dir-or-url>]`. Fetch only digest-pinned inputs (source archive of the tag, vendored dependency archive, builder image by digest) into a content-addressed cache and fail on any digest mismatch (RECIPE-DIGEST). Offline mode builds inside the pinned container with networking disabled and fails with RECIPE-NETWORK-REQUIRED if a step needs the network, then compares with the published artefacts using `rel repro-compare`. Put the recipe commands in fenced blocks tagged `rebuild`; add a CI job that runs them verbatim on a fresh Linux runner after each release for fid and fid-verify. Tests: AC-1..AC-3 of spec node ECO.REL.REPRO.RECIPE.

## Open questions

- Windows and macOS rebuilds need licensed OS images for third parties [unverified licensing]; the recipe covers Linux-built artefacts (CLI, verifier, web bundle) first.

## Children

_Leaf: none._
