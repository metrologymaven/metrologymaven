---
id: ECO.REPO.LAYERS
title: Crate layer map and layering check
level: 3
parent: ECO.REPO
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REPO-001, REQ-ECO.REPO-002, REQ-ECO.REPO-004, REQ-ECO.REPO-007]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REPO.LAYERS: Crate layer map and layering check

## Purpose

One file, `layers.toml`, assigns every first-party crate to exactly one layer in a fixed order (numerics at the bottom, apps and tools at the top), and one check fails the build when a crate reaches upward, directly or through other crates, or when a crate has no layer. This is the build-time enforcement of "the kernel depends on nothing above it" (CONSTITUTION §2).

## Scope

**In:**
- `layers.toml`: ordered layer list and crate-to-layer assignment
- `cargo xtask layers`: the check, using the build tool's resolved dependency graph
- Rules for dev-dependencies and the test-support layer
- Error output that names the offending path

**Out:**
- The third-party allowlist for the kernel graph (owned by `ECO.REPO.ALLOW`)
- Cross-repository pinning (owned by `ECO.REPO.TOPO`)
- Running the check on PRs (owned by `ECO.CI.GATES` and `ECO.CI.WF`)
- App-policy rules beyond layering (owned by `ECO.APPS.POLICY`)

## Requirements

- **REQ-ECO.REPO.LAYERS-001** `layers.toml` shall define an ordered list of layers and assign each first-party crate to exactly one layer; the initial order shall be `num`, `kernel`, `model`, `engine`, `services`, `viewer`, `app`, `tool`, plus a `test-support` layer usable only as a dev-dependency.
- **REQ-ECO.REPO.LAYERS-002** If a crate has a normal or build dependency path to a first-party crate in a higher layer, then the check shall fail and print the full path, one crate per hop.
- **REQ-ECO.REPO.LAYERS-003** If any crate depends on a crate in the `tool` layer, or a crate has a normal or build dependency on a crate in the `test-support` layer, then the check shall fail.
- **REQ-ECO.REPO.LAYERS-004** If a first-party crate in the workspace has no layer, or `layers.toml` names a crate that does not exist, then the check shall fail and name it.
- **REQ-ECO.REPO.LAYERS-005** Dev-dependencies shall follow the same upward rule as normal dependencies, except that any layer may use `test-support`.
- **REQ-ECO.REPO.LAYERS-006** The check shall run offline from the locked dependency graph and finish in under 60 s for a 200-crate workspace on the reference CI machine [unverified].

## Acceptance criteria

- **AC-1** Given fixture workspace `fixtures/layers/ok/` (crates `fid-num` in `num`, `fid-krn` in `kernel` depending on `fid-num`, `fid-app-x` in `app` depending on both), when `cargo xtask layers --workspace fixtures/layers/ok` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/layers/upward/` where `fid-krn` depends on `fid-viz` (layer `viewer`), when the check runs, then it exits 1 with `LAYER-UPWARD fid-krn -> fid-viz`.
- **AC-3** (error path) Given `fixtures/layers/transitive/` where `fid-krn` depends on third-party `shim`, which depends on first-party `fid-svc` (layer `services`), then it exits 1 and the printed path is `fid-krn -> shim -> fid-svc`.
- **AC-4** (error path) Given `fixtures/layers/unassigned/` with crate `fid-new` not in `layers.toml`, then it exits 1 with `LAYER-UNASSIGNED fid-new`; given `layers.toml` naming `fid-gone`, it exits 1 with `LAYER-UNKNOWN fid-gone`.
- **AC-5** Given `fixtures/layers/testsupport/` where `fid-krn` has a dev-dependency on `fid-testkit` (`test-support`), then it exits 0; if the same is a normal dependency, it exits 1 with `LAYER-TEST-SUPPORT`.
- **AC-6** (determinism) Output for AC-2 to AC-5 is byte-identical across two runs and three OSes.

## Design

- **Registry command(s):** none; repository tool `cargo xtask layers [--workspace <dir>] [--map layers.toml]`.
- **Data structures:** `layers.toml`: `order = [...]`, `[crates] "<name>" = "<layer>"`; graph from `cargo metadata --format-version 1 --locked --offline` (`resolve.nodes[].deps[].dep_kinds`).
- **Algorithm:** build a directed graph over packages with edge kinds; for each first-party crate, BFS over normal/build edges (and dev edges from the root only) keeping parent pointers; report the shortest path to each violating first-party crate; sort findings by (source, target).
- **Errors:** `LAYER-UPWARD`, `LAYER-TOOL`, `LAYER-TEST-SUPPORT`, `LAYER-UNASSIGNED`, `LAYER-UNKNOWN`, `LAYER-MAP` (bad file), `LAYER-METADATA`.
- **Determinism:** D0 (sorted traversal, BTreeMap).
- **Performance budget:** REQ-006.
- **Crate:** `fid-xtask`.

## Test fixtures

Fixture workspaces under `fixtures/layers/` with path dependencies only (no network), each with a committed `Cargo.lock`; a tiny third-party-like crate is simulated as a path crate not listed as first-party (first-party = workspace members).

## Build prompt

> In `fid-xtask` (implementation repository), add `layers [--workspace <dir>] [--map <file>]`. Read `layers.toml` (`order = ["num","kernel","model","engine","services","viewer","app","tool"]`, plus special `test-support`; `[crates]` name → layer). Run `cargo metadata --format-version 1 --locked --offline` in the workspace. First-party = workspace members. For every first-party crate, walk normal and build dependency edges transitively (dev edges only from the crate itself) and fail with `LAYER-UPWARD <shortest path>` on reaching a first-party crate of a higher layer; fail `LAYER-TOOL` on any dependency on a `tool` crate, `LAYER-TEST-SUPPORT` on a non-dev dependency on `test-support`, `LAYER-UNASSIGNED`/`LAYER-UNKNOWN` for missing or stale entries. Sorted findings, exit 0/1, offline. Seed `layers.toml` with the existing crates. Fixtures and tests for AC-1..AC-6 of spec node ECO.REPO.LAYERS. No `unwrap`/`expect`/`panic`/`unsafe` in non-test code.

## Open questions

- The layer names and order are a proposal from the baseline's crate structure [unverified against the CORE and KRN splits]; maintainers adjust them in `layers.toml` without code changes.
- D-005: TypeScript host shims, if any, are not crates and are outside this map.

## Children

_Leaf: none._
