---
id: GEO.SEL.MODEL
title: Index-set selections, digests and staleness
level: 3
parent: GEO.SEL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-geo-sel]
built_pr: 201
satisfies: [REQ-GEO.SEL-001, REQ-GEO.SEL-004]
covers: []
depends_on: [GEO.TYPES.DIGEST]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.SEL.MODEL: Index-set selections, digests and staleness

## Purpose

The data model of a selection: either a compact, digested set of indexes into one element (points, triangles, CAD faces or edges) bound to that element's digest, or a semantic query. It also guards against applying an index set to an element that has changed since the set was made.

## Scope

**In:**
- `IndexSet`: sorted, unique u64 indexes stored as ranges, with a domain (points, triangles, faces, edges)
- `Selection { target: EntityId, target_digest, body: IndexSet | Query(SelectionQuery) }`; the query type is defined by GEO.SEL.QUERY and held opaquely here
- Canonical encoding and SHA-256 digest of an index set
- `apply(selection, element)` with the stale-selection check
- Conversion from and to explicit index lists, with bounds checks

**Out:**
- Query language and evaluation (owned by `GEO.SEL.QUERY`)
- Pick and lasso resolution (owned by `GEO.SEL.RESOLVE`)
- Set algebra (owned by `GEO.SEL.ALGEBRA`)
- Commands (owned by `GEO.SEL.CMD`)

## Requirements

- **REQ-GEO.SEL.MODEL-001** An index set shall store sorted unique indexes as half-open ranges, and two index sets with the same members shall have the same encoding and digest.
- **REQ-GEO.SEL.MODEL-002** The index-set digest shall be SHA-256 over `FIDSEL`, a version byte, the domain byte, the 32-byte target element digest, the member count and the ranges as u64 little-endian pairs.
- **REQ-GEO.SEL.MODEL-003** If an index-set selection is applied to an element whose digest differs from the stored target digest, then `apply` shall fail with `StaleSelection { expected, found }`.
- **REQ-GEO.SEL.MODEL-004** If an index is not less than the element's count in the selection's domain, then construction shall fail with `IndexOutOfRange`.
- **REQ-GEO.SEL.MODEL-005** A selection shall contain no screen coordinates, camera parameters or GPU handles; its serialised form shall contain only the fields of this model.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/sel/sets.json` (index lists in random order with duplicates, and their expected range encodings and digests), when built, then encodings and digests match exactly on x86-64, ARM64 and wasm32.
- **AC-2** Given a selection on `fixtures/geo/types/pointset_small`, when applied to the same element, then it returns the members; when applied to a copy with one coordinate changed, then it fails with `StaleSelection`.
- **AC-3** (error path) Given index 1000 for a 1 000-point element, when constructed, then `IndexOutOfRange`.
- **AC-4** Given 10⁸ indexes forming 10 ranges, when stored, then memory is O(ranges) (under 1 KiB).
- **AC-5** A schema test shows the serialised `Selection` has no camera, pixel or GPU fields.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Domain { Points, Triangles, Faces, Edges }`; `IndexSet { domain, ranges: Vec<(u64, u64)> }`; `Selection`; `SelectionDigest([u8; 32])`.
- **Algorithm:** sort and deduplicate, then coalesce into ranges; membership by binary search; digest by streaming the canonical encoding.
- **Errors:** `StaleSelection`, `IndexOutOfRange`, `DomainMismatch`.
- **Determinism:** D0.
- **Performance budget:** building from 10⁷ unsorted indexes under 1 s [unverified].
- **Crate:** `fid-geo-sel` (kernel graph, KNP-1 applies).
- **Pinned value-affecting choices** (as built in `fid-geo-sel`):
  - Domain bytes: points 1, triangles 2, faces 3, edges 4. Digest preimage `"FIDSEL" | 0x01 | domain u8 | target element digest [32] | count u64 | (start u64, end u64)*`, all little-endian; `count` is the number of members; ranges are half-open, non-empty, ascending and coalesced (touching ranges merge); the range count is not written. SHA-256 is the portable FIPS 180-4 code of `fid-kci`; the target digest is the GEO.TYPES.DIGEST `ElementDigest` from `fid-geo-types`.
  - Input lists may be in any order and may repeat an index; a repeat is one member (set semantics), not an error. The empty set is valid. Explicit ranges must have `start < end` (`InvalidRange`).
  - Bounds: every constructor takes the element's count in the domain; an index `>= count` is `IndexOutOfRange { index, count }` (the first offending index in input order; for an explicit range, `end - 1`). From an element: points of a point set or cloud and mesh vertices are `Points`; mesh triangles are `Triangles`; `Faces` and `Edges` exist only on a CAD body handle and their count comes from GEO.CAD topology (not in GEO.TYPES), so those sets are built with an explicit count. Any other pairing is `DomainMismatch`.
  - `apply` checks, in order: query body (`NotIndexSet`, queries are GEO.SEL.QUERY's), element digest (`StaleSelection { expected, found }`), domain (`DomainMismatch`), bound (`IndexOutOfRange`; a decoded selection is first bound-checked here). For CAD faces and edges only digest and domain are checked.
  - `EntityId` is carried as its 16 bytes (the CORE UUID type is above the kernel layer). The query body is opaque canonical bytes, at most 64 KiB.
  - Serialised selection: `"FIDSLC" | 0x01 | target [16] | target_digest [32] | body tag u8`, then for tag 1 `domain u8 | n_ranges u64 | ranges`, for tag 2 `len u32 | query bytes`. Decoding rejects non-canonical ranges, unknown tags, truncation and trailing bytes, and bounds `n_ranges` by the input length before allocating.
  - Conversion to an explicit list takes a caller limit (`TooManyIndices`). The builder merges ascending runs as they arrive, so memory is O(ranges) for run-ordered input; stored sets are shrunk to fit.

## Test fixtures

- `fixtures/geo/sel/sets.json`: seeded index lists (seed 71) with digests computed by an independent Python script. CC-BY-4.0.

## Build prompt

> Create crate `fid-geo-sel` (pure Rust, no I/O, kernel graph, KNP-1) with module `model`: `Domain { Points, Triangles, Faces, Edges }`, `IndexSet { domain, ranges: Vec<(u64,u64)> }` (sorted, unique, half-open, coalesced), `Selection { target: EntityId, target_digest: ElementDigest, body: IndexSet | Query(opaque query value) }`, digest SHA-256 over `FIDSEL | 0x01 | domain | target digest | count u64 | ranges as u64 LE pairs`, `apply(&Selection, &GeometryElement) -> Result<&IndexSet, SelError>` failing with `StaleSelection{expected, found}` when the element digest differs, and bounds checks (`IndexOutOfRange`). No camera, pixel or GPU data anywhere in the model. Acceptance: AC-1 encodings and digests match `sets.json` on three targets; AC-2 stale detection; AC-3 bounds; AC-4 range-compact memory; AC-5 schema has no view fields. Constraints: CONSTITUTION C2, §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-sel/src/model.rs`, `tests/model.rs`, `tools/fixtures/geo_sel_sets.py`.

## Open questions

- None.

## Children

_Leaf: none._
