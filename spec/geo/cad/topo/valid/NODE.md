---
id: GEO.CAD.TOPO.VALID
title: B-rep defect detection without repair
level: 4
parent: GEO.CAD.TOPO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TOPO-004, REQ-GEO.CAD.TOPO-005]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.ADJ, GEO.CAD.TOPO.PCURVE, GEO.NURBS.INVERT, GEO.NURBS.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TOPO.VALID: B-rep defect detection without repair

## Purpose

Checks a built body against the topological and geometric conditions the rest of GEO relies on and lists every defect by kind and entity ID, so users and importers see exactly what is wrong. It never changes the body.

## Scope

**In:**
- Defect kinds: `OpenLoop`, `SelfIntersectingLoop`, `DanglingEdge`, `NonManifoldEdge`, `InconsistentOrientation`, `EdgeOffSurface`, `PcurveMismatch`, `PcurveFailed`, `SeamWinding`
- One tolerance in metres for gaps and deviations (default 1e-7 m [unverified])
- Report sorted by (defect kind order, entity ID), with measured values
- An optional cap on reported defects with a `truncated` count

**Out** (and which node owns it instead):
- Repair or healing (not offered; a future repair command would be a separate node; D-024 places healing in an optional import plugin)
- The command wrapper (owned by `GEO.CAD.TOPO.CMD`)

## Requirements

- **REQ-GEO.CAD.TOPO.VALID-001** If consecutive coedges of a loop leave a 3D gap above tolerance, then validation shall report `OpenLoop { loop, gap_m }`.
- **REQ-GEO.CAD.TOPO.VALID-002** If a loop's trimming polyline intersects itself or another loop of the same face, then validation shall report `SelfIntersectingLoop { loop }`.
- **REQ-GEO.CAD.TOPO.VALID-003** If an edge is used by no coedge or by only one coedge in a body declared solid, then validation shall report `DanglingEdge { edge, uses }`, and if used by more than two faces in a solid, `NonManifoldEdge { edge, uses }`.
- **REQ-GEO.CAD.TOPO.VALID-004** If the two coedges of a manifold edge in a solid traverse it in the same direction relative to their face senses, then validation shall report `InconsistentOrientation { edge }`.
- **REQ-GEO.CAD.TOPO.VALID-005** If an edge curve deviates from an adjacent face surface by more than tolerance at any of 33 sample parameters, then validation shall report `EdgeOffSurface { edge, face, deviation_m }`; if an imported pcurve disagrees with its edge curve by more than tolerance, `PcurveMismatch { coedge, deviation_m }`.
- **REQ-GEO.CAD.TOPO.VALID-006** Validation shall take the body by shared reference and return a report only; it shall never modify, heal or rebuild the body.
- **REQ-GEO.CAD.TOPO.VALID-007** The report shall list defects in a fixed order (kind order, then ascending entity ID), so the same body gives the same report bytes on every platform.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/topo/cube_labelled.json`, when validated with 1e-7 m, then the report is empty.
- **AC-2** Given fixture set `fixtures/geo/cad/topo/defects/` (one body per defect kind, each with exactly one seeded defect), when validated, then each report contains exactly the expected defect with the expected entity ID and measured value within 1e-12 m.
- **AC-3** Given any body from AC-2, when validated, then its `body_digest` is unchanged afterwards.
- **AC-4** Given a body with 50 defects and a cap of 20, then 20 defects are listed in fixed order and `truncated = 30`.
- **AC-5** Given AC-2 on three OSes, then the reports are byte-identical in canonical JSON.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Defect` enum as in Scope with measured values; `ValidationReport { tolerance_m, defects: Vec<Defect>, truncated: u64 }`.
- **Algorithm:** edge-use counts from GEO.CAD.TOPO.ADJ; loop gaps from evaluated coedge endpoints; self-intersection by a sweep over trimming polyline segments sorted by (min u, segment index) with exact orientation predicates; edge-to-surface distance by GEO.NURBS.INVERT of 33 edge samples onto each adjacent surface; pcurve disagreement from GEO.CAD.TOPO.PCURVE.
- **Errors:** `BadTolerance`; defects are data, not errors.
- **Determinism:** D0.
- **Performance budget:** validation of a 10⁴-face body under 2 s single-threaded [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `fixtures/geo/cad/topo/defects/`: nine hand-edited JSON body descriptions derived from the cube and plate fixtures, one defect each, with expected reports. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, KNP-1, no I/O), add module `topo::valid` with `validate(&Body, tol_m, cap: Option<u64>) -> Result<ValidationReport, ValidError>`. Detect `OpenLoop`, `SelfIntersectingLoop`, `DanglingEdge`, `NonManifoldEdge`, `InconsistentOrientation`, `EdgeOffSurface` (33 samples, surface inversion), `PcurveMismatch`, and pass through `PcurveFailed` and `SeamWinding` from `topo::pcurve`; each defect carries its entity IDs and measured value. Sort by kind order then ascending ID; apply the cap with a `truncated` count. Take `&Body` only and never modify it. Acceptance: AC-1 clean cube; AC-2 one fixture per defect kind; AC-3 digest unchanged; AC-4 cap; AC-5 byte-identical reports on three OSes. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (exact orientation predicates, no FMA, no HashMap). Files: `crates/fid-geo-cad/src/topo/valid.rs`, `tests/topo_valid.rs`, `fixtures/geo/cad/topo/defects/`.

## Open questions

- The default tolerance (1e-7 m) needs evidence from IO.CORP STEP corpora [unverified].

## Children

_Leaf: none._
