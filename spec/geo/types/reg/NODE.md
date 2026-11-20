---
id: GEO.TYPES.REG
title: Element registration and describe command
level: 3
parent: GEO.TYPES
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-geo-cmd]
satisfies: [REQ-GEO.TYPES-007, REQ-GEO.TYPES-008]
covers: []
depends_on: [GEO.TYPES.DIGEST, CORE.PRJ, CORE.REG, CORE.DEP]
side_effect: R0
determinism: D0
cli: [fid geo describe-element]
mcp: [geo_describe_element]
journal: [geo.describe_element@1]
updated: 2026-11-16
---

# GEO.TYPES.REG: Element registration and describe command

## Purpose

The bridge between the pure geometry types and the document: a registration helper that every GEO-producing command calls to put an element into the CORE element tree under a stable ID, and the read-only `geo.describe_element@1` command that reports what an element is.

## Scope

**In:**
- `register_element` and `replace_element` helpers used inside other commands' transactions
- Stable-ID rules per CONSTITUTION §5 for geometric elements
- `geo.describe_element@1` (R0): kind, counts, extents, frame, digest, attribute channels, mesh topology facts

**Out:**
- The element tree and its storage (owned by `CORE.PRJ`)
- Dependency recompute (owned by `CORE.DEP`)
- Commands that create geometry (owned by each producer: `IO`, `SCAN`, `GEO.FRAME.CMD` and others)

## Requirements

- **REQ-GEO.TYPES.REG-001** When an element is registered with an imported source ID, the registration shall keep that ID unchanged.
- **REQ-GEO.TYPES.REG-002** When an element is registered as new with no source ID, the registration shall request a UUIDv7 from CORE, and when it is registered as derived it shall use UUIDv5 over the producing command ID and the sorted input element IDs and digests.
- **REQ-GEO.TYPES.REG-003** When an element is replaced, the registration shall keep its ID, store the new digest and notify CORE.DEP of the change in the same transaction.
- **REQ-GEO.TYPES.REG-004** When `geo.describe_element@1` runs on an element ID, the command shall return JSON with kind, point and triangle counts, AABB in metres, frame ID, digest hex, attribute channel names and, for meshes, topology facts.
- **REQ-GEO.TYPES.REG-005** If `geo.describe_element@1` is given an ID that does not exist or is not geometric, then the command shall fail with `not_found` or `not_geometry` and change nothing.
- **REQ-GEO.TYPES.REG-006** The describe command shall be class R0 and shall be recorded only in the Activity Log, never as a Record Journal entry.

## Acceptance criteria

- **AC-1** Given a project with fixture `fixtures/geo/types/pointset_small` registered, when `fid geo describe-element <id> --json` runs, then the output has `kind: "PointSet"`, `points: 1000`, the fixture's AABB, frame ID and the digest from `digest_vectors.json`.
- **AC-2** Given `mesh_cube` registered, when described, then the JSON includes `topology.closed: true` and `triangles: 12`.
- **AC-3** (error path) Given an unknown UUID, when described, then the command fails with `not_found`, the Record Journal length is unchanged, and one Activity Log line is written.
- **AC-4** Given the same derived element produced twice from the same command and inputs, when registered, then both get the same UUIDv5; an imported element registered with source ID `step:#123` keeps it.
- **AC-5** Given a registered element replaced by a different mesh, when registration runs, then the ID is unchanged, the digest is updated and CORE.DEP reports the element's dependents `outdated` (using a mock dependency graph in the test).
- **AC-6** The MCP tool `geo_describe_element` is present in the generated manifest with projection `exposed`.

## Design

- **Registry command(s):** `geo.describe_element@1`, class R0, params `{ element: EntityRef }`, result `ElementDescription`; determinism D0; projected to MCP as exposed.
- **Data structures:** `enum IdOrigin { Imported(String), New, Derived { command: CommandId, inputs: Vec<(EntityId, ElementDigest)> } }`; `ElementDescription { kind, points, triangles, aabb, frame, digest, channels, topology }`.
- **Algorithm:** UUIDv5 namespace is a fixed project constant; name bytes are the command ID followed by input IDs sorted ascending with their digests.
- **Errors:** `not_found`, `not_geometry`.
- **Determinism:** D0.
- **Performance budget:** describe under 5 ms for any element (it reads cached facts, never rescans) [unverified].
- **Crate:** `fid-geo-cmd` (GEO command adapter, outside the kernel graph).

## Test fixtures

- Fixtures from GEO.TYPES.POINTS, GEO.TYPES.MESH and GEO.TYPES.DIGEST; an in-memory test project from CORE's test harness.

## Build prompt

> In crate `fid-geo-cmd` (the GEO command adapter; it may depend on CORE crates and on `fid-geo-types`, but never the reverse), implement: (a) `register_element(tx, element, origin: IdOrigin) -> EntityId` and `replace_element(tx, id, element)`, using the CORE element-tree and dependency-graph APIs inside the caller's transaction; IDs follow CONSTITUTION §5: imported IDs preserved, new IDs UUIDv7 from CORE, derived IDs UUIDv5 over the command ID plus sorted input IDs and digests; replacement keeps the ID, stores the new digest and notifies CORE.DEP. (b) Registry command `geo.describe_element@1`, class R0, MCP projection exposed, determinism D0, params `{element}`, result `{kind, points, triangles, aabb, frame, digest, channels, topology}`; errors `not_found` and `not_geometry`; Activity Log only. CLI `fid geo describe-element <id> [--json]` and MCP `geo_describe_element` are generated from the registry, not hand-written. Acceptance: AC-1..AC-6 (describe of point set and cube; unknown ID fails with no journal entry; deterministic UUIDv5 and imported-ID preservation; replace keeps ID and marks dependents outdated; tool present in the manifest). Constraints: CONSTITUTION §3 naming, §5 IDs, §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/register.rs`, `src/describe.rs`, `tests/describe.rs`.

## Open questions

- The CORE.PRJ element-tree API is not finalised; this leaf assumes a transaction handle with `insert`, `replace` and `get`.

## Children

_Leaf: none._
