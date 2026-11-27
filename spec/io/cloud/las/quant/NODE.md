---
id: IO.CLOUD.LAS.QUANT
title: Scaled-integer coordinates, writer scale and offset, quantisation metadata
level: 4
parent: IO.CLOUD.LAS
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-io-las]
satisfies: [REQ-IO.CLOUD.LAS-001, REQ-IO.CLOUD.LAS-002, REQ-IO.CLOUD.LAS-003]
covers: []
depends_on: [IO.CLOUD.LAS.HDR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.LAS.QUANT: Scaled-integer coordinates and quantisation

## Purpose

Owns the precision rules for LAS, LAZ and COPC: converting stored integers to f64 coordinates with exactly one rounding, attaching each axis's scale, offset and quantisation uncertainty q/√12 to the point set for UNC, and, on export, choosing scale and offset and refusing any coordinate that cannot be represented.

## Scope

**In:**
- Read: x = offset + scale × X computed with error-free transformations (Dekker split product and two-sum, no FMA) and rounded once
- Quantisation metadata per axis: `scale`, `offset`, `u_quant = scale / sqrt(12)` (sqrt via `fid-num`)
- Write: scale per axis from the caller (must be ≤ 1e-7 m) or default 1e-7 m; offset = the axis midpoint rounded to a multiple of the scale; range check of every coordinate against i32 before any byte is written
- Export report: chosen scale and offset, maximum quantisation error actually incurred per axis

**Out:**
- Record layout (`IO.CLOUD.LAS.HDR`), compression (`IO.CLOUD.LAS.LAZ`)
- How UNC uses the metadata (UNC)

## Requirements

- **REQ-IO.CLOUD.LAS.QUANT-001** Imported coordinates shall equal the correctly rounded value of offset + scale × integer for every axis.
- **REQ-IO.CLOUD.LAS.QUANT-002** Every imported point set shall carry per-axis scale, offset and quantisation uncertainty scale/√12 as metadata.
- **REQ-IO.CLOUD.LAS.QUANT-003** If a caller-supplied write scale exceeds 1e-7 m on any axis, then the export shall fail with `scale_too_coarse` naming the axis.
- **REQ-IO.CLOUD.LAS.QUANT-004** If any coordinate is not representable as a 32-bit integer at the chosen scale and offset, then the export shall fail with `out_of_range` naming the first such point index and axis, before writing any byte.
- **REQ-IO.CLOUD.LAS.QUANT-005** The export report shall state the scale, offset and maximum incurred quantisation error per axis.

## Acceptance criteria

- **AC-1** Given 10^6 seeded (X, scale, offset) triples, when converted, then each result equals a reference computed with 256-bit arbitrary-precision arithmetic and rounded once.
- **AC-2** Given a point set imported with scale 0.001, then metadata holds `u_quant` = 0.001/√12 computed via `fid-num`.
- **AC-3** (error path) Given a 500 m long cloud at scale 1e-7 m, then `out_of_range{index, axis}` and no bytes are written.
- **AC-4** (error path) Given caller scale 1e-6 m, then `scale_too_coarse{axis: x}`.
- **AC-5** Given a 10 m cloud exported with defaults, then the report shows scale 1e-7 m and max quantisation error ≤ 5e-8 m per axis.

## Design

- **Registry commands:** none (library).
- **Data structures:** `AxisQuant { scale, offset, u_quant }`; `QuantPlan { axes: [AxisQuant; 3] }`; `QuantReport { max_err: [f64; 3] }`.
- **Algorithm:** Dekker split (factor 2^27 + 1) for exact products, Knuth two-sum, single final rounding; offset = round_to_multiple((min + max)/2, scale); two passes on export (range check, then encode) or one pass with spill when the source is chunked.
- **Errors:** `out_of_range{index, axis}`, `scale_too_coarse{axis}`.
- **Determinism:** D0 (KNP-1: no FMA, `fid-num` sqrt).
- **Performance budget:** ≥ 200 M conversions/s [unverified until measured].
- **Crate:** `fid-io-las`.

## Test fixtures

Seeded generators; arbitrary-precision reference in tests only.

## Build prompt

> In crate `fid-io-las`, implement scaled-integer conversion: x = offset + scale × X rounded once using error-free transformations (Dekker split product + two-sum; no FMA, KNP-1); attach per-axis scale, offset and `u_quant = scale/sqrt(12)` (`fid-num`) as point-set metadata. For export, accept a caller scale ≤ 1e-7 m per axis (`scale_too_coarse{axis}` otherwise; default 1e-7 m), choose offset as the axis midpoint rounded to a scale multiple, range-check every coordinate before writing (`out_of_range{index, axis}`), and report scale, offset and max incurred error. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-5 (arbitrary-precision reference in tests only).

## Open questions

- REQ-IO.CLOUD-009 caps the scale at 1e-7 m, which limits a LAS export to about ±214 m around the offset; larger sites must use E57. Flagged for the IO.CLOUD owner.
- Build notes (`fid-io-las::quant`): "rounded once" is implemented as correct rounding of the three-term sum offset + product-high + product-low with a round-to-odd step (Boldo and Melquiond 2008); the fixture holds cases where the naive two-rounding formula is wrong. An exactness envelope bounds hostile headers and export input: scale in [2^-512, 2^512] and positive, |offset| and |coordinate| at most 2^480 (`scale_invalid`, `offset_invalid`, `coordinate_outside_envelope`); NaN or infinite export coordinates are `non_finite`, an empty export is `empty_point_set` (no midpoint). Export integers are the nearest to the exact (c - offset) / scale, ties to even and counted per axis in the report. The report's `max_err` is the error a reader of the file sees, `|c - decode(X)|`; with the binary64 1e-7 scale it can exceed 5e-8 m only within about one ulp of a half-way case. AC-1's reference is exact (rationals in the Python oracle; an exact 128-bit integer reference over the seeded generator's range in the Rust test) rather than 256-bit floating point. Whether the import command (`IO.CLOUD.LAS.CMD`) refuses or reports a header outside the envelope is that leaf's call.

## Children

_Leaf: none._
