---
id: GDT.DIM.SECT.SPAN
title: 2D element extraction from a section-curve span
level: 4
parent: GDT.DIM.SECT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-gps]
built_pr: 678
satisfies: [REQ-GDT.DIM.SECT-001, REQ-GDT.DIM.SECT-002]
covers: [GOM.DIM.07]
depends_on: [KRN.LSQ, GEO.FRAME]
uses_artifacts: [SCAN.SECT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DIM.SECT.SPAN: 2D element extraction from a section-curve span

## Purpose

Turns a named span of a section curve into a 2D element in the section plane frame: a line, a circle (arc) or a point, fitted with the KRN fit the resolved chain names. The section curve itself comes from SCAN as a published element format (plane, ordered polyline, section ID and digest); this leaf reads that format and never computes a section. Its output carries the provenance that makes later staleness detection possible.

## Scope

**In:** reading the section-curve element format published by `SCAN.SECT` (read as an artifact, because SCAN is a later L2 domain than GDT); `CurveSpan { section_id, start, end }` with start and end as arc-length parameters in metres along the curve; selecting the span's vertices (inclusive bounds, curve order); fitting `Line2d`, `Circle2d` or `Point2d` (centroid or a single vertex) with the chain-named KRN algorithm in the plane frame; `SectionElement { kind, params_2d, plane, section_id, section_digest, span, fit_alg }`.
**Out:** computing sections (owned by `SCAN.SECT`); the checks on the extracted elements (owned by `GDT.DIM.SECT.CMDS`); staleness (owned by `GDT.DIM.SECT.STALE`); fits (owned by `KRN.LSQ`).

## Requirements

- **REQ-GDT.DIM.SECT.SPAN-001** Given a section curve and a span, the function shall select exactly the curve vertices whose arc-length parameter lies in [start, end] and fit the element kind the chain names with the chain-named KRN algorithm in the section plane frame.
- **REQ-GDT.DIM.SECT.SPAN-002** The returned `SectionElement` shall record the section ID, the section digest, the plane, the span and the fit algorithm ID.
- **REQ-GDT.DIM.SECT.SPAN-003** If the span selects fewer vertices than the element kind needs (2 for a line, 3 for a circle, 1 for a point) or start ≥ end, then the function shall return `span_too_short{needed, got}` or `bad_span`.
- **REQ-GDT.DIM.SECT.SPAN-004** If the span parameters lie outside the curve's arc-length range, then the function shall return `span_out_of_range`.
- **REQ-GDT.DIM.SECT.SPAN-005** Fitted analytic spans shall recover the generating line and circle within 1e-12 m for noise-free fixtures.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/sect/rect_with_fillet.json` (a closed 2D polyline of a 20 × 10 mm rectangle with a 2 mm fillet, spacing 0.05 mm), when spans over a straight edge and over the fillet are extracted, then the line lies on the edge and the circle has radius 2 mm within 1e-12 m.
- **AC-2** (error path) Given a span holding 2 vertices for a circle, then `span_too_short{needed: 3, got: 2}`.
- **AC-3** (error path) Given end beyond the curve length, then `span_out_of_range`.
- **AC-4** Given AC-1, then each element records the section ID and digest from the fixture.
- **AC-5** (determinism) Output bits are identical on all CI platforms.

## Design

- **Registry command(s):** none (kernel function `extract_section_element(curve: &SectionCurveView, span, kind, chain) -> Result<SectionElement, SpanError>`).
- **Data structures:** `SectionCurveView` (read-only view of the SCAN section format: plane, ordered 2D vertices, closed flag, section ID, digest, schema version); `CurveSpan`; `SectionElement`.
- **Algorithm:** cumulative arc length in fixed order; inclusive selection; for closed curves a span may wrap (start > end means wrap only when `closed` and an explicit `wrap: true` is set; otherwise `bad_span`); call the KRN 2D fit.
- **Errors:** `span_too_short{needed, got}`, `bad_span`, `span_out_of_range`, `unsupported_section_schema{version}`, pass-through fit errors.
- **Determinism:** D0.
- **Performance budget:** 100 000-vertex curve < 5 ms per span [unverified].
- **Crate:** `fid-gps` (module `dim::section`).

## Test fixtures

Analytic 2D polylines generated in-repo (CC0): rectangle with fillets, slot, circle.

## Build prompt

> In crate `fid-gps` (kernel, pure), module `dim::section`, implement `extract_section_element(curve, span, kind, chain)`. `curve` is a read-only view of the section-curve format that SCAN publishes (plane, ordered 2D vertices, closed flag, section ID, digest, schema version; reject unknown versions with `unsupported_section_schema`). Select vertices whose cumulative arc length lies in [start, end] (inclusive; wrap only for closed curves with explicit `wrap: true`), then fit `Line2d`, `Circle2d` or `Point2d` with the KRN least-squares 2D fit the chain names. Return `SectionElement { kind, params_2d, plane, section_id, section_digest, span, fit_alg }`. Errors: `span_too_short{needed, got}`, `bad_span`, `span_out_of_range`. Tests per acceptance criteria with analytic fixtures (1e-12 m). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- The section-curve format and its schema version must be named in `SCAN.SECT`'s Interfaces (D-034 publisher duty). **Until then this leaf defines the format it reads, provisionally:** `fid.scan.sect.curve/1` in `fid-gps` `dim::section::curve` (section ID, a required `fid-geo-frame` frame ID, the plane as origin and two orthonormal in-plane axes, ordered `[u, v]` vertices as IEEE-754 bit texts, the closed flag, and a digest over the canonical JSON of every other field, recomputed on load). SCAN does not publish it yet; `SCAN.SECT` should adopt it or name its own, and this reader follows.
- **Choices made at build time (confirm in review).** The span carries the section ID and an explicit `wrap` flag (Design); a span naming another section is `section_mismatch`. Only the least-squares fits of `fid-fit` on main are run (`fit.line2.ls`, `fit.circle2.ls`, `fit.point.ls`); a chain naming another method or version is `fit_unavailable`, a chain without exactly one association step is `chain_without_association` / `ambiguous_association`, and a fit of the wrong primitive is `fit_kind_mismatch`. `Point2d` is the centroid of the selection (one vertex gives that vertex). Span bounds are compared exactly (no tolerance); a vertex's arc-length parameter is in `[0, length)`, so on a closed curve vertex 0 is selected by `start = 0` only. Zero-length segments and segments below the rounding resolution of the running arc length are refused when the curve is built. AC-5 is a regression pin of the output bits (Linux, Windows, macOS, and wasm32 through `ci/wasm32-crates.txt`), not an oracle.

## Children

_Leaf: none._
