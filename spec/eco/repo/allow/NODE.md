---
id: ECO.REPO.ALLOW
title: Kernel-graph dependency allowlist
level: 3
parent: ECO.REPO
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.REPO-003]
covers: []
depends_on: [ECO.REPO.LAYERS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REPO.ALLOW: Kernel-graph dependency allowlist

## Purpose

The kernel computes every value of record (C1), so every third-party crate it pulls in, directly or transitively, must be deliberately chosen. `kernel-allowlist.toml` lists the third-party crates and version ranges allowed anywhere in the dependency graph of the `num` and `kernel` layers, and the check fails on anything else.

## Scope

**In:**
- `kernel-allowlist.toml`: crate name, allowed version requirement, reason, reviewer
- `cargo xtask kernel-allow`: compute the third-party closure of kernel-layer crates and compare

**Out:**
- Layer assignments and upward-dependency rules (owned by `ECO.REPO.LAYERS`)
- Licence checks (owned by `ECO.CI.LIC`)
- KNP-1 numeric-profile lints (owned by `KRN`)

## Requirements

- **REQ-ECO.REPO.ALLOW-001** If any third-party crate in the normal or build dependency closure of a crate in the `num` or `kernel` layer is not listed in `kernel-allowlist.toml`, then the check shall fail and print a dependency path to it.
- **REQ-ECO.REPO.ALLOW-002** If a listed crate's resolved version does not satisfy the entry's version requirement, then the check shall fail and name both.
- **REQ-ECO.REPO.ALLOW-003** Every allowlist entry shall have a non-empty `reason` and `reviewer`, or the check shall fail.
- **REQ-ECO.REPO.ALLOW-004** If an allowlist entry is not reached by any kernel-layer crate, then the check shall warn that the entry is unused.

## Acceptance criteria

- **AC-1** Given `fixtures/allow/ok/` (`fid-krn` depends on path crate `ext-a 1.2.0`, allowlisted as `ext-a = "^1.2"`), when `cargo xtask kernel-allow --workspace fixtures/allow/ok` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/allow/unlisted/` (`ext-a` depends on `ext-b`, not listed), then it exits 1 with `ALLOW-UNLISTED ext-b via fid-krn -> ext-a -> ext-b`.
- **AC-3** (error path) Given `fixtures/allow/version/` (`ext-a 2.0.0` resolved, entry `^1.2`), then it exits 1 with `ALLOW-VERSION ext-a 2.0.0 ^1.2`.
- **AC-4** (error path) Given an entry without `reviewer`, then it exits 1 with `ALLOW-SCHEMA`.
- **AC-5** Given `fixtures/allow/unused/` (an entry for `ext-z` not reached), then it exits 0 and prints `warn ALLOW-UNUSED ext-z`.
- **AC-6** Given a crate in the `viewer` layer depending on an unlisted crate, then it exits 0 (only the kernel graph is checked).

## Design

- **Registry command(s):** none; repository tool `cargo xtask kernel-allow [--workspace <dir>]`.
- **Data structures:** `[[allow]] name, version, reason, reviewer`; layer map from ECO.REPO.LAYERS.
- **Algorithm:** reuse the LAYERS graph loader; collect kernel-layer roots; BFS over normal/build edges; classify non-member packages; compare names and versions (semver requirement matching).
- **Errors:** `ALLOW-UNLISTED`, `ALLOW-VERSION`, `ALLOW-SCHEMA`, warning `ALLOW-UNUSED`.
- **Determinism:** D0.
- **Performance budget:** under 5 s on top of metadata loading [unverified].
- **Crate:** `fid-xtask`.

## Test fixtures

Fixture workspaces with path crates standing in for third-party crates under `fixtures/allow/`, each with a committed `Cargo.lock`.

## Build prompt

> In `fid-xtask`, add `kernel-allow [--workspace <dir>]`, reusing the dependency-graph loader from the `layers` command (ECO.REPO.LAYERS). Read `kernel-allowlist.toml` (`[[allow]] name, version (semver req), reason, reviewer`). Roots: workspace crates in layers `num` and `kernel`. Walk normal and build edges; for each non-workspace package reached, fail `ALLOW-UNLISTED <name> via <path>` if unlisted and `ALLOW-VERSION` if its version does not match; fail `ALLOW-SCHEMA` for empty reason/reviewer; warn `ALLOW-UNUSED` for unreached entries. Sorted output, exit 0/1, offline. Seed the file empty (the kernel should start with no third-party dependencies beyond those maintainers approve). Fixtures and tests for AC-1..AC-6 of spec node ECO.REPO.ALLOW. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Whether platform-specific dependencies (target-gated) count: proposed yes, all targets, so the allowlist does not vary by OS.

## Children

_Leaf: none._
