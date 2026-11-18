---
id: CORE.BUS.LINT
title: No-side-doors crate-graph check
level: 3
parent: CORE.BUS
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
satisfies: [REQ-CORE.BUS-016]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.BUS.LINT: No-side-doors crate-graph check

## Purpose

A CI check that makes "no side doors" (CONSTITUTION C3) structural: it reads the workspace crate graph and fails the build if any crate other than the bus and storage crates can reach mutable project state or the database driver, or if a surface crate depends on anything but the bus API.

## Scope

**In:** reading the workspace dependency graph, an allowlist file, transitive reachability checks, deterministic failure reports.
**Out:** the list of crates allowed to write project files is supplied by `CORE.STO` as an input file; GUI binding audits (`VIZ`); surface parity checks (`AUTO`, `ECO`).

## Requirements

- **REQ-CORE.BUS.LINT-001** The check shall read the workspace crate graph, including transitive dependencies, from the build tool's metadata output.
- **REQ-CORE.BUS.LINT-002** If any crate not listed in the allowlist file depends, directly or transitively, on the mutable-state crate, then the check shall fail and print the dependency path.
- **REQ-CORE.BUS.LINT-003** If any crate not listed as a storage writer depends, directly or transitively, on the SQLite driver crate, then the check shall fail and print the dependency path.
- **REQ-CORE.BUS.LINT-004** If a crate marked as a surface (CLI, MCP, Python, GUI, control channel) depends on any CORE crate other than the bus API crate, then the check shall fail.
- **REQ-CORE.BUS.LINT-005** The check shall print its findings sorted by crate name, so two runs on the same workspace produce identical output.
- **REQ-CORE.BUS.LINT-006** The check shall complete in under 30 s on a workspace of 200 crates [unverified until measured].

## Acceptance criteria

- **AC-1** Given fixture workspace `fixtures/core/bus/lint/clean/` (bus, store, api, cli, kernel crates with allowed edges), when `cargo xtask lint-crate-graph` runs, then it exits 0.
- **AC-2** Given fixture workspace `fixtures/core/bus/lint/side_door/` where `cli` depends on `fid-state` through an intermediate crate, when the check runs, then it exits 1 and prints the path `cli -> helper -> fid-state`.
- **AC-3** Given fixture workspace `fixtures/core/bus/lint/sqlite_leak/` where a viewer crate depends on the SQLite driver, then the check exits 1 naming that crate.
- **AC-4** Given any fixture, when the check runs twice, then the outputs are byte-identical.

## Design

- **Registry command(s):** none; a build-time tool run as `cargo xtask lint-crate-graph` in CI.
- **Data structures:** `lint-allowlist.toml` with `[mutable_state] allowed = [...]`, `[storage_writers] allowed = [...]` (provided by CORE.STO), `[surfaces] crates = [...]`, `bus_api = "fid-bus-api"`.
- **Algorithm:** parse `cargo metadata --format-version 1`; build an ordered adjacency map; breadth-first search from each crate to the protected crates, recording the first path found in lexicographic order.
- **Errors:** `metadata_unavailable`, `allowlist_invalid`.
- **Determinism:** D0; ordered maps and lexicographic traversal.
- **Performance budget:** see REQ-006.
- **Crate:** `fid-xtask` (workspace tooling crate).

## Test fixtures

Three minimal Cargo workspaces under `fixtures/core/bus/lint/` (`clean`, `side_door`, `sqlite_leak`) with empty crate bodies; CC-BY-4.0 / MIT OR Apache-2.0 like the code.

## Build prompt

> In the workspace tooling crate `fid-xtask`, add the subcommand `lint-crate-graph`. Read `cargo metadata --format-version 1` and `lint-allowlist.toml`. Fail (exit 1) when: a crate outside `mutable_state.allowed` reaches the mutable-state crate; a crate outside `storage_writers.allowed` reaches the SQLite driver crate; a crate listed in `surfaces.crates` depends on any `fid-` CORE crate other than the bus API crate. Print each finding with its dependency path, sorted by crate name. Use ordered maps (`BTreeMap`) and lexicographic BFS for deterministic output. Tests: AC-1..AC-4 using the fixture workspaces in `fixtures/core/bus/lint/`. Constraints: CONSTITUTION §8 (no panic, deny unwrap/expect), allowlisted dependencies; wire it into CI in a separate CI PR per AGENTS.md.

## Open questions

- None.

## Children

_Leaf: none._
