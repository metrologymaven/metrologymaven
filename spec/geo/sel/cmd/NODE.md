---
id: GEO.SEL.CMD
title: Selection commands
level: 3
parent: GEO.SEL
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL-001, REQ-GEO.SEL-002, REQ-GEO.SEL-007]
covers: []
depends_on: [GEO.SEL.MODEL, GEO.SEL.RESOLVE, GEO.SEL.ALGEBRA, GEO.TYPES.REG, CORE.REG, CORE.JRN]
side_effect: W2
determinism: D0
cli: [fid geo create-selection, fid geo resolve-selection, fid geo combine-selection, fid geo grow-selection, fid geo describe-selection]
mcp: [geo_create_selection, geo_resolve_selection, geo_combine_selection, geo_grow_selection, geo_describe_selection]
journal: [geo.create_selection@1, geo.resolve_selection@1, geo.combine_selection@1, geo.grow_selection@1, geo.describe_selection@1]
built_in: [fid-geo-cmd]
updated: 2026-11-23
---

# GEO.SEL.CMD: Selection commands

## Purpose

The registry commands that create, resolve, combine and grow selections as document elements, so every selection a later command uses is journaled as geometry (an index set and its digest), never as screen state.

## Scope

**In:**
- `geo.create_selection@1` (W2): from an explicit index list
- `geo.resolve_selection@1` (W2): from camera parameters plus a pick or lasso, resolved on the CPU before the entry is written
- `geo.combine_selection@1` (W2): union, intersection, difference, symmetric difference, invert
- `geo.grow_selection@1` (W2): grow or shrink by rings or radius
- `geo.describe_selection@1` (R0): domain, count, ranges count, target, digests
- Registration of selections as derived elements with dependency edges to their target

**Out:**
- Semantic query selections and re-evaluation commands (owned by `GEO.SEL.QUERY`)
- Library logic (owned by `GEO.SEL.MODEL`, `GEO.SEL.RESOLVE`, `GEO.SEL.ALGEBRA`)

## Requirements

- **REQ-GEO.SEL.CMD-001** The four mutating commands shall be class W2 and produce exactly one Record Journal entry each; `geo.describe_selection@1` shall be R0.
- **REQ-GEO.SEL.CMD-002** The journal entry of `geo.resolve_selection@1` shall record the resulting index-set digest and target digest; the camera and polygon shall be recorded only as the command's arguments, and the stored selection shall contain neither.
- **REQ-GEO.SEL.CMD-003** If a selection used as input is stale against its target, then the command shall fail with `stale_selection` and journal nothing.
- **REQ-GEO.SEL.CMD-004** When a selection is created, it shall be registered as an element derived from its target, so replacing the target marks it `outdated`.
- **REQ-GEO.SEL.CMD-005** Explicit index lists given inline shall be limited to 10⁶ indexes per call; larger sets come from resolution, algebra or queries.

## Acceptance criteria

- **AC-1** Given a project with `fixtures/geo/types/pointset_small`, when `fid geo create-selection <id> --indices 1-10,20 --json` runs, then a selection element exists with 11 members, its digest matches GEO.SEL.MODEL, and the journal grows by one entry.
- **AC-2** Given a lasso case from `fixtures/geo/sel/lasso_cases`, when `fid geo resolve-selection <mesh> --camera cam.json --lasso poly.json --json` runs, then the stored selection equals the library result and its serialised form contains no camera or pixel fields.
- **AC-3** Given two selections, when `fid geo combine-selection --op union A B --json` runs, then the result equals GEO.SEL.ALGEBRA's union and A and B are unchanged.
- **AC-4** (error path) Given a selection whose target mesh was replaced, when combined, then `stale_selection` is returned and the journal is unchanged.
- **AC-5** (replay) Given the journal from AC-1 to AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-6** The manifest lists the four W2 tools as named tools and `geo_describe_selection` as exposed.

## Design

- **Registry command(s):** `geo.create_selection@1` W2 `{ target, domain, indices }`; `geo.resolve_selection@1` W2 `{ target, domain, camera, pick | lasso, rule?, mode? }`; `geo.combine_selection@1` W2 `{ op, a, b? }`; `geo.grow_selection@1` W2 `{ selection, rings? + adjacency? | radius? (authored length), shrink? }`; `geo.describe_selection@1` R0 `{ selection }`. All return the new selection ID and digest.
- **Data structures:** schemas only; selections are CORE elements.
- **Algorithm:** validate, call the library, register via GEO.TYPES.REG.
- **Errors:** `stale_selection`, `element_mismatch`, `domain_mismatch`, `bad_camera`, `bad_polygon`, `too_many_indices`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** adapter overhead under 2 ms [unverified].
- **Crate:** `fid-geo-cmd`.

## Test fixtures

- GEO.SEL fixtures and a CORE test project.

## Build prompt

> In crate `fid-geo-cmd`, register five commands (CLI and MCP generated from the manifest; W2 calls by agents commit to their branch): `geo.create_selection@1` (W2, explicit indices, ≤ 10⁶), `geo.resolve_selection@1` (W2, camera plus pick or lasso resolved on the CPU by `fid-geo-sel::resolve`; the stored selection holds only the index set and digests), `geo.combine_selection@1` (W2, set operations from `algebra`), `geo.grow_selection@1` (W2, rings or authored-length radius, grow or shrink) and `geo.describe_selection@1` (R0). Register every new selection as derived from its target (dependency edge, so target replacement marks it outdated); reject stale inputs with `stale_selection`. Acceptance: AC-1..AC-6 (create with one journal entry; resolve without view fields stored; combine leaves inputs unchanged; stale input refused; exact replay `IDENTICAL`; tool projections). Constraints: CONSTITUTION C2, §3, §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/sel_cmds.rs`, `tests/sel_cmds.rs`.

## Open questions

- None.

## Children

_Leaf: none._
