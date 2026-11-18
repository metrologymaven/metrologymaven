---
id: GDT.PROF.ZDEF.NONUNIF
title: Non-uniform profile zone intervals
level: 4
parent: GDT.PROF.ZDEF
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.PROF.ZDEF-001, REQ-GDT.PROF.ZDEF-002]
covers: []
depends_on: [GDT.PROF.ZDEF.UNIF, GDT.PROF.DEV, GEO.CAD.TOPO]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.ZDEF.NONUNIF: Non-uniform profile zone intervals

## Purpose

Builds per-point allowed intervals for a non-uniform profile zone, where the zone boundaries are stated at defined points (or as offset boundary geometry) along the feature and vary between them. How the boundary varies between the stated points, and along which parameter of a surface, is standards interpretation that a single scalar does not settle [unverified]; the rule must come from a signed-off DMT row. This node stays open until such a row exists, then becomes a leaf that fills the `PerPoint` variant of `ZoneIntervals`.

## Scope

**In:**
- Reading the stated boundary points or boundary offsets from the characteristic
- Locating each deviation's foot point along the feature (curve parameter or surface parameterisation) through GEO
- Interpolating the lower and upper boundary at each foot point with the method the row names
- The `zone_definition` record fragment for non-uniform zones (stated points, method ID)

**Out** (and which node owns it instead):
- Constant-width definitions and dispatch (owned by `GDT.PROF.ZDEF.UNIF`)
- Foot points and face IDs (owned by `GDT.PROF.DEV`)
- Which interpolation a callout implies (owned by DMT rows in `GDT.RES.ROWASME` and `GDT.RES.ROWISO`)

## Requirements

- **REQ-GDT.PROF.ZDEF.NONUNIF-001** When a characteristic states a non-uniform zone, the system shall return one allowed interval per deviation, interpolated between the stated boundary points by the method the resolved row names.
- **REQ-GDT.PROF.ZDEF.NONUNIF-002** If a foot point cannot be located between two stated boundary points (for example it lies beyond the last stated point), then the system shall flag that deviation `outside_defined_span` and exclude it from the value, unless the row states an extrapolation rule.
- **REQ-GDT.PROF.ZDEF.NONUNIF-003** The `ZoneDefRecord` for a non-uniform zone shall list the stated points, their boundary offsets in metres and the interpolation method ID.
- **REQ-GDT.PROF.ZDEF.NONUNIF-004** If the row names no interpolation method, then the system shall return `UNSUPPORTED{non_uniform}`.

## Interfaces

- **Offers:** the `PerPoint` arm of `zone_intervals` (GDT.PROF.ZDEF.UNIF dispatches to it).
- **Consumes:** deviations and foot points (GDT.PROF.DEV); face parameterisation and topology (GEO.CAD.TOPO); the interpolation method named in the resolved chain.

## Decomposition hints

Likely one leaf once the row exists: CURVE (interpolation along a line element) and SURF (along a surface) may need to be separate if the surface rule needs a parameterisation choice.

## Open questions

- Blocked on expert content, not on a decision: no signed-off DMT row yet defines the interpolation between stated points (linear in arc length, linear in a face parameter, or a boundary surface from CAD) [unverified]. Route to the DMT authors under D-009; do not guess.
- If the drawing defines the zone by explicit boundary geometry in the CAD model instead of stated points, the input is a second nominal surface; confirm the PMI representation with `GDT.PMI.MAP`.

## Children

_None yet: status `open`._
