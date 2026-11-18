---
id: ECO.REPO.TOPO
title: Repository map and cross-repository pin check
level: 3
parent: ECO.REPO
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
satisfies: [REQ-ECO.REPO-005]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REPO.TOPO: Repository map and cross-repository pin check

## Purpose

Records which repositories exist and what each holds (`repos.toml`), and checks that every reference from one first-party repository to another is pinned to an immutable revision, so a change in a library repository can never silently change a build of an app repository.

## Scope

**In:**
- `repos.toml`: repository ID, URL, purpose, whether its tags are protected
- `cargo xtask pins`: check git dependencies in every `Cargo.toml` against first-party repository URLs

**Out:**
- Pinning shared CI workflow references (owned by `ECO.CI.WF`)
- When to create a second code repository or promote a library (a maintainer decision; this node only records the outcome)
- Repo template (owned by `ECO.REPO.TPL`)

## Requirements

- **REQ-ECO.REPO.TOPO-001** `repos.toml` shall list each first-party repository with an ID, canonical URL, one-line purpose and a `protected_tags` flag.
- **REQ-ECO.REPO.TOPO-002** If a git dependency on a first-party repository uses `branch` or no revision selector, then the pin check shall fail and name the manifest file and dependency.
- **REQ-ECO.REPO.TOPO-003** If a git dependency on a first-party repository uses `tag` and that repository's `protected_tags` is false, then the pin check shall fail; a full 40-character `rev` shall always pass.
- **REQ-ECO.REPO.TOPO-004** Git URLs shall be compared after normalisation (scheme, trailing `.git`, case of host), so a differently spelled URL of a first-party repository is still checked.

## Acceptance criteria

- **AC-1** Given `fixtures/pins/ok/` (a dependency on a first-party URL with a 40-hex `rev`, and one with `tag` on a repository with `protected_tags = true`), when `cargo xtask pins --workspace fixtures/pins/ok --repos fixtures/pins/repos.toml` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/pins/branch/` (`branch = "main"`), then it exits 1 with `PIN-MOVING <file> <dep>`.
- **AC-3** (error path) Given `fixtures/pins/tag-unprotected/`, then it exits 1 with `PIN-TAG-UNPROTECTED`.
- **AC-4** (error path) Given `fixtures/pins/url-variant/` using `HTTPS://Example.org/fid/libs` (no `.git`) with `branch`, then it exits 1 with `PIN-MOVING`.
- **AC-5** Given a git dependency on a non-first-party URL with `branch`, then this check exits 0 (third-party sources are the licence and supply-chain checks' concern).

## Design

- **Registry command(s):** none; repository tool `cargo xtask pins [--workspace <dir>] [--repos <file>]`.
- **Data structures:** `[[repo]] id, url, purpose, protected_tags`; initial entries: the spec repository and the code monorepo.
- **Algorithm:** parse every workspace `Cargo.toml` (members and root) with a bounded TOML reader; for `dependencies`, `dev-dependencies`, `build-dependencies` and target-specific tables, inspect `git`, `rev`, `tag`, `branch`; normalise URLs; apply rules.
- **Errors:** `PIN-MOVING`, `PIN-TAG-UNPROTECTED`, `PIN-REV-SHORT` (rev not 40 hex), `PIN-REPOS` (bad map).
- **Determinism:** D0.
- **Performance budget:** under 1 s for 200 manifests [unverified].
- **Crate:** `fid-xtask`.

## Test fixtures

Fixture workspaces with `Cargo.toml` files only (no fetch needed) and a `repos.toml` under `fixtures/pins/`.

## Build prompt

> In `fid-xtask`, add `pins [--workspace <dir>] [--repos <file>]`. Read `repos.toml` (`[[repo]] id, url, purpose, protected_tags`). Parse each workspace `Cargo.toml` (bounded, no network) including target-specific dependency tables; for each `git` dependency whose normalised URL (lower-case scheme and host, strip trailing `.git` and `/`) matches a first-party repo: fail `PIN-MOVING` if it has `branch` or no `rev`/`tag`; fail `PIN-REV-SHORT` if `rev` is not 40 hex; fail `PIN-TAG-UNPROTECTED` if it uses `tag` on a repo with `protected_tags = false`. Sorted findings, exit 0/1. Seed `repos.toml` with the spec repo and the code monorepo. Fixtures and tests for AC-1..AC-5 of spec node ECO.REPO.TOPO. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Until there is more than one code repository, the check passes trivially in the monorepo; it matters from the first library split (a maintainer call).

## Children

_Leaf: none._
