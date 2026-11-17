---
id: GDT.DIM
title: Dimensional and size checks
level: 2
parent: GDT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT-003, REQ-GDT-007]
covers: [GOM.DIM.*, PW.GDT.01, PCD.DIM.01, SA.GDT.02]
depends_on: [GDT.CHAR, GDT.RES, KRN, ALN.MODEL]
cli: [fid check *-distance, fid check *-angle, fid check *-diameter, fid check *-radius, fid check *-caliper, fid check *-minmax, fid check *-deviation, fid check *-size, fid check *-scalar]
mcp: [check_*_distance, check_*_angle, check_*_diameter, check_*_radius, check_*_caliper, check_*_minmax, check_*_deviation, check_*_size, check_*_scalar]
journal: [check.*_distance, check.*_angle, check.*_diameter, check.*_radius, check.*_caliper, check.*_minmax, check.*_deviation, check.*_size, check.*_scalar]
updated: 2026-11-16
---

# GDT.DIM: Dimensional and size checks

## Purpose

Evaluates the "plain" dimensional characteristics that are not geometric tolerances: distances, angles, diameters and radii, caliper checks, min/max checks, scalar checks on element properties, checks on sections, point deviations, and feature-of-size evaluation (ASME Rule #1 envelope and two-point local size in P1a; ISO 14405-1 size modifiers in P1b). These are the most common checks in every incumbent and the first ones users try, so they form one subsystem with one evaluation pattern.

## Scope

**In:**
- Point-to-point and element-to-element distances (including projected and per-axis components)
- Angles between elements
- Diameter and radius of circles, cylinders, spheres, cones
- Caliper checks (two-point extent along a direction)
- Scalar checks on element properties (for example a fitted feature's length or cone angle)
- Checks on section elements (values computed from a section curve supplied by SCAN)
- Min and max checks over a set of values or deviations
- Point deviations in x, y, z and along the nominal normal; feature-vs-CAD-nominal checks
- Feature-of-size evaluation: ASME Y14.5-2018 two-point local size and Rule #1 envelope (P1a); ISO 14405-1 size modifiers LP, LS, GG, GX, GN, GC, CC, CA, CV and rank-order modifiers per edition (P1b)
- Legacy (non-GD&T) location and distance dimensions as in CMM software

**Out** (and which node owns it instead):
- Fitting features (owned by `KRN`)
- Alignment and the coordinate frame (owned by `ALN`)
- Section curve and deviation-map computation (owned by `SCAN`)
- Geometric tolerances with zones (owned by `GDT.FORM`, `GDT.LOC`, `GDT.PROF`)
- Operator chain selection (owned by `GDT.RES`)
- ResultRecord assembly and decision binding (owned by `GDT.CHAR`)

## Requirements

- **REQ-GDT.DIM-001** The system shall evaluate each dimensional check type listed in Scope from either measured points or fitted features, and the ResultRecord shall record which (`POINTS` or `FEATURES`).
- **REQ-GDT.DIM-002** Each dimensional check shall report its value in the characteristic's authored unit, with the SI value retained and the single journaled conversion rounding of CONSTITUTION §4.
- **REQ-GDT.DIM-003** Distance checks shall state their measurand explicitly (3D, projected onto a named plane, or a named axis component of a named frame), and a distance check without a stated measurand shall be rejected.
- **REQ-GDT.DIM-004** Point deviations shall be reported per point as dx, dy, dz in the named frame and as the signed normal deviation, with the sign convention (material side positive) stated in the result.
- **REQ-GDT.DIM-005** When an ASME Y14.5-2018 feature of size is evaluated, the system shall report both the minimum and maximum two-point local size and the Rule #1 envelope check (actual mating envelope via KRN minimum-circumscribed or maximum-inscribed fit), and shall flag `local_optimum_only` when KRN flags the envelope fit as non-convex.
- **REQ-GDT.DIM-006** When an ISO 14405-1 size modifier is evaluated (P1b), the system shall tag the result with the ISO 14405-1 edition used and shall refuse a modifier that the DMT marks unsupported for that edition.
- **REQ-GDT.DIM-007** Every dimensional evaluation shall match the published reference values of its ORS-SIZE or RDL cases within 1e-9 mm for analytic cases [unverified: tolerance to be confirmed against oracle precision].

## Interfaces

- **Offers:** commands `check.eval_<type>@1` (R1 scratch, W2 when committed to the run) for each type in Scope; dimensional result payloads into the ResultRecord.
- **Consumes:** fitted features and constructions (KRN); frames (ALN); section curves and deviation values (SCAN); operator chains (GDT.RES); characteristic and ResultRecord builder (GDT.CHAR).

## Decomposition hints

LINEAR (distance, caliper, point deviation), ANGULAR, RADIAL (diameter, radius), SCALAR (element properties, section values, min/max), SIZE-ASME, SIZE-ISO.

## Open questions

- Checks on sections depend on SCAN (phase P1b); resolved by giving them their own child `GDT.DIM.SECT` at P1b.
- `GDT.DIM.SIZEASME` and `GDT.DIM.SIZEISO` are not leaves: pairing rules, envelope definitions and modifier definitions are DMT content that awaits expert rows (D-009, D-016).

## Split rationale

The dividing principle is **measurand family**, following the hints: linear (distance, caliper), angular, radial, scalar and min/max, point and feature deviations, then size per dialect. Sections are split out by phase (SCAN is P1b). The plain checks need no expert interpretation beyond a trivial DMT row, so they are finished as leaves; the size children carry the interpretation-heavy content and stay open.

Alternatives considered:
- **One leaf per seed row (nine children).** Rejected: distance, element distance and caliper share one computation; separating them would duplicate the pair table.
- **Sections inside SCALAR.** Rejected: different phase and an external dependency on `SCAN.SECT`.
- **One SIZE child for both dialects.** Rejected: different phases, row sets and source texts.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.DIM.LINEAR | Distance and caliper checks (leaf) | 001 002 003 007 | GOM.DIM.01, GOM.DIM.02, GOM.DIM.05, PW.GDT.01, PCD.DIM.01 |
| GDT.DIM.ANGLE | Angle checks between elements (leaf) | 001 002 007 | GOM.DIM.03, PW.GDT.01, PCD.DIM.01 |
| GDT.DIM.RADIAL | Diameter and radius checks (leaf) | 001 002 007 | GOM.DIM.04, PW.GDT.01 |
| GDT.DIM.SCALAR | Scalar property and min/max checks (leaf) | 001 002 007 | GOM.DIM.06, GOM.DIM.08 |
| GDT.DIM.SECT | Checks on section elements | 001 002 007 | GOM.DIM.07 |
| GDT.DIM.PTDEV | Point deviations and feature-versus-nominal checks (leaf) | 001 002 004 007 | GOM.DIM.09, SA.GDT.02 |
| GDT.DIM.SIZEASME | ASME feature-of-size: two-point size and Rule #1 envelope | 001 005 007 | PW.GDT.01 |
| GDT.DIM.SIZEISO | ISO 14405-1 size modifiers | 001 006 007 | none |
