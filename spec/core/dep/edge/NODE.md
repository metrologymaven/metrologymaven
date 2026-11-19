---
id: CORE.DEP.EDGE
title: Dependency edges, cycle rejection and dependency queries
level: 3
parent: CORE.DEP
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-dep]
built_pr: 223
satisfies: [REQ-CORE.DEP-001, REQ-CORE.DEP-002]
covers: []
depends_on: [CORE.PRJ.TREE, CORE.BUS]
side_effect: R0
determinism: D0
cli: [fid project show-dependencies]
mcp: [project_show_dependencies]
journal: [project.show_dependencies@1]
updated: 2026-11-16
---

# CORE.DEP.EDGE: Dependency edges, cycle rejection and dependency queries

## Purpose

Records, at commit time, which entities each element was computed from, refuses any commit that would create a cycle, and answers "what does this depend on, and what depends on it?". The edges are part of the committed events, so they are journaled, replayable and rebuilt on reconstruction like any other state.

## Scope

**In:** deriving edges from a command's entity-reference parameters (`x-ref`) and its output elements; replacing an element's incoming edges on update; cycle detection before commit; the `project.show_dependencies@1` query; the in-memory edge index.
**Out:** marking dependents outdated (`CORE.DEP.PROP`); recompute (`CORE.DEP.RECOMP`); input replacement (`CORE.DEP.REPL`); the element tree (`CORE.PRJ.TREE`).

## Requirements

- **REQ-CORE.DEP.EDGE-001** When a command creates or updates an element, the dependency graph shall add, in the same commit, one edge from each entity referenced by the command's `x-ref` parameters to that element.
- **REQ-CORE.DEP.EDGE-002** When a command updates an element, the dependency graph shall replace that element's incoming edges with the edges of the new computation.
- **REQ-CORE.DEP.EDGE-003** If the edges of a commit would create a cycle, then the dependency graph shall reject the commit with a typed `cycle` error listing the cycle's element IDs, starting from the smallest ID, and commit nothing.
- **REQ-CORE.DEP.EDGE-004** When `project.show_dependencies@1` runs for an element, the system shall return its direct dependencies and direct dependents, each sorted by entity ID.
- **REQ-CORE.DEP.EDGE-005** When `project.show_dependencies@1` runs with `transitive` set, the system shall return all transitive dependencies and dependents, each sorted by entity ID.
- **REQ-CORE.DEP.EDGE-006** When a project is reconstructed from the journal, the edge index shall equal the index that existed when the head was committed.
- **REQ-CORE.DEP.EDGE-007** Adding the edges of one commit to a graph of 100,000 elements, including the cycle check, shall take under 10 ms on reference hardware [unverified until measured].

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/core/dep/chain5` (test elements a → b → c → d → e of the test-kit type `num`), when `fid project show-dependencies c --json` runs, then dependencies are `[b]` and dependents `[d]`; with `--transitive`, `[a, b]` and `[d, e]`.
- **AC-2** Given the same project, when a command updates `a` to take `e` as input, then the command fails with `cycle` listing `[a, b, c, d, e]` and the journal head is unchanged.
- **AC-3** Given an update of `c` that now takes `a` instead of `b`, when committed, then `c`'s dependencies are `[a]` and `b`'s dependents are `[]`.
- **AC-4** Given the journal after AC-3, when the project is closed and reopened, then the edge index is identical (compared by a canonical digest of the sorted edge list).
- **AC-5** Given `fixtures/core/dep/wide_100k` (100,000 elements), when one more element with 10 inputs is created, then edge insertion plus cycle check takes under 10 ms in the benchmark.

## Design

- **Registry command(s):** `project.show_dependencies@1` R0; params `{ element: EntityRef, transitive: bool = false }`; result `{ dependencies: [EntityId], dependents: [EntityId] }`.
- **Data structures:** `EdgeIndex { forward: BTreeMap<EntityId, BTreeSet<EntityId>>, reverse: BTreeMap<EntityId, BTreeSet<EntityId>> }`; event `EdgesSet { element, inputs: [EntityId] }` appended by the bus commit hook.
- **Algorithm:** at commit, collect `x-ref` values from params; for each output element, check reachability from the element to any new input with an iterative DFS over `forward` (visiting in ID order so the reported cycle is deterministic); on success emit `EdgesSet`.
- **Errors:** `cycle`, `not_found`.
- **Determinism:** D0; ordered maps, ID-ordered traversal.
- **Performance budget:** REQ-007.
- **Crate:** `fid-dep` (module `edge`).

Pinned at build time (`fid-dep::edge`):

- **IDs:** `EntityId` is `fid_uuid::Uuid`; text form is the 36-character lowercase hyphenated UUID only (any other spelling is refused). ID order is byte order of the UUID.
- **Edges:** an edge `input -> element` means `element` was computed from `input`. `forward` maps input to dependents, `reverse` maps element to inputs. `reverse` has one entry per known element (possibly empty); `forward` holds no empty set. An element becomes known through its first `EdgesSet` (a source has `inputs: []`).
- **Event:** journal event type `dep.edges_set@1`; payload `{"element": id, "inputs": [id, ...]}` with `inputs` strictly ascending. A repeated input is `duplicate_edge`, a descending pair `unsorted_inputs`, any other key, missing key or non-canonical ID `bad_event`.
- **Edge derivation (REQ-001, REQ-002):** the hook walks the parameter schema and the parameters together through `properties` and `items` only; a value at an `x-ref` position must be a canonical ID string (`bad_reference` otherwise, with its JSON path). A schema with an `x-ref` anywhere else (e.g. `anyOf`, `additionalProperties`, `$defs`) or on a non-string type is refused with `unsupported_schema`, so no edge is silently missed. Each output gets one edge per distinct referenced entity; an `x-ref` that names one of the command's own outputs names the target and is not an input. The new set replaces the output's incoming edges.
- **Create and update:** `Output { element, op: Create | Update }`. A create of a known element is `already_exists`; an update of an unknown one is `not_found/unknown_element`; an output listed twice is `duplicate_element`. An input that is not a known element is `not_found/dangling_input`.
- **Commit:** sets are applied in ascending element order, each checked against the index as it stands (so an input must be known before its set). The commit is all or nothing: on any error every set already applied is rolled back and no event is returned. Returned events are in that order and are what the bus appends.
- **Cycle check (REQ-003):** an element listing itself is `cycle [x]`. Otherwise an iterative DFS from the element over `forward`, children in ascending ID order, stops at the first new input reached; the DFS path `[x, ..., input]` is the cycle, rotated to start at its smallest ID and kept in edge direction.
- **Queries (REQ-004, REQ-005):** `dependencies` from `reverse`, `dependents` from `forward`, transitive closures by iterative DFS; all lists ascending; the element itself is never listed; unknown element is `not_found/unknown_element`. Params are refused with `bad_params` on an unknown key, a missing or non-canonical `element` or a non-boolean `transitive`.
- **Reconstruction (REQ-006):** `EdgeIndex::rebuild` applies the journaled events in journal order through the same per-set check; it fails closed on the first invalid event. Index identity is the SHA-256 (`fid-types` `Digest`) of the RFC 8785 bytes of `{"edges": [[input, element], ...], "elements": [...]}`, both ascending.
- **Bounds:** at most 4096 inputs per element and 4096 elements per commit; the schema and parameter walkers stop at depth 64 and 65,536 JSON nodes (counted before pushing). Exceeding any is `input_limit`. No recursion anywhere.
- **Errors:** closed set with stable `code()`/`reason()`: `cycle`, `not_found` (`unknown_element`, `dangling_input`), `already_exists`, `duplicate_element`, `duplicate_edge`, `unsorted_inputs`, `bad_reference`, `unsupported_schema`, `bad_params`, `bad_event`, `input_limit`, `internal`. Mapping onto the bus catalogue (`CORE.BUS.PIPE.ERR`, which has no `cycle` code yet) is left to the bus integration.
- **Registry:** `command_def()` builds the `project.show_dependencies@1` `CommandDef` (R0, D0, natural idempotency, not undoable, exposed on CLI and MCP) through `CommandDef::from_value`; `element` carries `x-ref: element`.
- **Fixtures:** `fixtures/core/dep/chain5/project.txt` and `wide_100k/recipe.txt` are hand-written; `make_fixtures.py` (stdlib reference implementation, `--check`) writes `expected.txt` for each. `wide_100k` is generated from its recipe (SplitMix64, seed 20261119, 1 to 3 older inputs per element) rather than committed as 100,000 lines.

## Test fixtures

Test-kit element type `num` (value = sum of inputs, registered only in tests) and generated projects `fixtures/core/dep/chain5` and `wide_100k`, produced by a committed script with a fixed seed; CC-BY-4.0.

## Build prompt

> In crate `fid-dep`, add module `edge`. Provide a commit hook that, for every committed command creating or updating elements, derives edges from the command's `x-ref` parameters to each output element, replaces the element's incoming edges, rejects cycles with `cycle` (element IDs listed from the smallest), and emits an `EdgesSet` event in the same commit. Keep a forward and reverse `BTreeMap<EntityId, BTreeSet<EntityId>>` index rebuilt from `EdgesSet` events on reconstruction. Register `project.show_dependencies@1` (R0; params element, transitive=false; sorted direct or transitive dependencies and dependents). CLI `fid project show-dependencies`, MCP `project_show_dependencies`. Use iterative traversals only. Tests: AC-1..AC-5 with a test-kit `num` element type and generated fixtures under `fixtures/core/dep/`. Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect), no HashMap.

## Open questions

- Parameters that are not entities (for example a project-level nominal value) become dependency sources only if they are modelled as entities; this leaf assumes they are.

## Children

_Leaf: none._
