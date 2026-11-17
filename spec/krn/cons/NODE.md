---
id: KRN.CONS
title: Constructions and live relationships
level: 2
parent: KRN
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN-004, REQ-KRN-005, REQ-KRN-007]
covers: [GOM.CON.01, GOM.CON.02, GOM.CON.03, GOM.CON.04, GOM.CON.05, GOM.CON.06, GOM.CON.07, GOM.CON.08, GOM.CON.09, GOM.CON.10, GOM.CON.11, GOM.CON.12, GOM.CON.13, GOM.CON.14, GOM.CON.15, GOM.CON.16, GOM.CON.17, GOM.CON.22, PCD.CON.01, PCD.CON.03, PW.CON.04, SA.REL.02, SA.REL.03]
depends_on: [KRN.LSQ, KRN.MAN, GEO]
cli: [fid construct *]
mcp: [construct_*]
journal: [construct.*]
updated: 2026-11-16
---

# KRN.CONS: Constructions and live relationships

## Purpose

Builds features from other features or from geometry, rather than directly from points: intersection, projection, offset, midpoint, symmetry, bisector, best-fit of features, constructed sets, and the curves and points derived from nominal geometry. It also provides live relationships (point-to-object and object-to-object distances and angles) as constructions that recompute. Each construction is a pure kernel function plus a declaration of its inputs, so the CORE dependency graph can recompute it when an input changes.

## Scope

**In:**
- Construction operations: intersect (line-line, line-plane, plane-plane, plane-cylinder, circle-circle and similar pairs), project (point, line, circle onto plane or line), offset, midpoint and mid-plane, symmetry element, bisector
- Construct point, line, plane, circle, slot, rectangle, cylinder, cone and sphere from other elements (by construction, or by best fit of feature centres through `KRN.LSQ`)
- Constructed sets: ordered groups of features used as one input
- Derived curves and points: section curves through a cutting plane, edge and trim curves, and surface points on nominal geometry
- System planes, axes and coordinate-system elements as constructions
- Automatic construction of elements from CAD faces, with nominal-to-actual pairing rules
- Live relationships: point-to-object and object-to-object distance, angle and deviation, with optional nominal and tolerance attributes for reporting
- Input-dependency declarations for each construction (which element IDs and parameters it reads)
- Typed errors for undefined constructions (parallel planes, skew lines without a defined closest-point rule, coincident inputs)
- Spec cards and algorithm IDs `construct.<op>.<variant>@<semver>`

**Out** (and which node owns it instead):
- The dependency graph and recompute scheduling (`CORE`)
- Fitting from points (`KRN.LSQ`, `KRN.MZ`, `KRN.ENV`)
- Point-to-mesh and point-to-CAD projection primitives and NURBS evaluation (`GEO`)
- Section comparison, whiskers and deviation maps on scan data (`SCAN`)
- Conformance decisions on relationship tolerances (`UNC`) and their presentation in reports (`RPT`)
- Alignment and coordinate-system solving from measured features (`ALN`)

## Requirements

- **REQ-KRN.CONS-001** The subsystem shall provide intersection, projection, offset, midpoint, symmetry and bisector constructions for every pair of point, line, plane, circle, cylinder, cone and sphere where the result is geometrically defined.
- **REQ-KRN.CONS-002** If a construction is geometrically undefined or ill-conditioned (for example, parallel planes or nearly coincident inputs beyond the configured angular threshold), then it shall return a typed error with the condition value and no element.
- **REQ-KRN.CONS-003** Each construction shall be a pure function of its declared inputs and parameters, and shall publish those inputs as a dependency declaration that the CORE dependency graph can consume.
- **REQ-KRN.CONS-004** When any declared input of a construction changes, recomputing the construction shall give the same result bit for bit as constructing it fresh from the new inputs.
- **REQ-KRN.CONS-005** The subsystem shall construct point, line, plane, circle, slot, rectangle, cylinder, cone and sphere elements from other elements, including best fit through feature centres or a constructed set.
- **REQ-KRN.CONS-006** The subsystem shall derive section curves, edge and trim curves and surface points from nominal geometry, and system planes and coordinate-system elements, as constructions.
- **REQ-KRN.CONS-007** When the user asks to construct elements automatically from CAD faces, the subsystem shall create one nominal element per eligible face and shall pair it with actual data by a documented, parameterised rule.
- **REQ-KRN.CONS-008** The subsystem shall provide point-to-object and object-to-object relationships (distance, angle, deviation) as constructions that recompute through the dependency graph and carry optional nominal and tolerance attributes.
- **REQ-KRN.CONS-009** Each construction algorithm shall have an algorithm ID `construct.<op>.<variant>@<semver>` and a spec card.
- **REQ-KRN.CONS-010** When a construction command is committed, the system shall journal one `construct.*` event with algorithm ID, input element IDs, parameter digest and output digest.

## Interfaces

- **Offers:** commands `construct.<verb_object>@1` (for example intersect, project, offset, midpoint, symmetry, bisector, relate); CLI `fid construct *`; MCP `construct_*`; journal `construct.*`; dependency declarations for `CORE`; relationship values for `GDT`, `RPT` and `LVM`.
- **Consumes:** `KRN.LSQ` (fit through centres), `KRN.MAN` (spec cards), `GEO` (types, frames, projection to mesh and CAD, NURBS evaluation).

## Decomposition hints

OPS (intersect, project, offset, midpoint, symmetry, bisector), ELEM (construct each primitive from elements, constructed sets), DERIV (sections, edges, trims, surface points, system planes), AUTO (construction from CAD with nominal-actual pairing), REL (live relationships).

## Open questions

- Section curves and edge or trim curves overlap with `SCAN` (section comparison, edge deviation). This node owns the construction of the curve as an element; `SCAN` owns comparison against it. Confirm with the SCAN expansion.
- The kernel depends on nothing above it, yet constructions must recompute through `CORE`. This node assumes the declaration is data the kernel emits and `CORE` consumes, with no crate dependency from the kernel to `CORE`.
- Phase: the concept baseline puts constructions in P1a [unverified for relationships, which SpatialAnalyzer users need from P3].

## Split rationale

The split follows **what a construction is built from**, with the shared contract first. DECL fixes the construction contract (purity, dependency declaration as data, typed errors, journal shape) that every other child implements, which answers the layering question in Open questions: the kernel emits declarations as data and `CORE.DEP` consumes them. Then:
- OPS: geometric operations on existing elements (intersect, project, offset, midpoint, symmetry, bisector);
- ELEM: building a primitive from several elements, including best fit through feature centres and constructed sets;
- DERIV: elements derived from nominal geometry (sections, edges, trims, surface points) and system planes and coordinate systems;
- AUTO: automatic construction from CAD faces with nominal-actual pairing, which starts with CAD in P1b;
- REL: live relationships, which are constructions whose output is a value (distance, angle, deviation) rather than a feature.

Alternatives considered:
- **One child per output primitive** (construct point, construct line...). Rejected: each would repeat every operation; the seeds list both views, so ELEM covers the per-primitive seeds and OPS the per-operation ones.
- **Relationships outside KRN** (in `GDT` or `RPT`). Rejected: they must recompute through the dependency graph like any construction; only their reporting is elsewhere.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.CONS.DECL | Construction contract and dependency declarations | 002 003 004 009 010 | none |
| KRN.CONS.OPS | Construction operations on element pairs | 001 002 004 009 010 | GOM.CON.14-17, PCD.CON.01, PW.CON.04 |
| KRN.CONS.ELEM | Primitives built from elements and constructed sets | 005 009 010 | GOM.CON.01-09, PCD.CON.01, PCD.CON.03, PW.CON.04 |
| KRN.CONS.DERIV | Derived curves, surface points and system elements | 006 009 010 | GOM.CON.10-13 |
| KRN.CONS.AUTO | Automatic construction from CAD with nominal-actual pairing | 007 009 010 | GOM.CON.22 |
| KRN.CONS.REL | Live relationships | 008 009 010 | SA.REL.02, SA.REL.03 |
