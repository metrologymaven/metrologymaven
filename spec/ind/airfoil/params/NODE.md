---
id: IND.AIRFOIL.PARAMS
title: Airfoil section parameters
level: 3
parent: IND.AIRFOIL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.AIRFOIL-003, REQ-IND.AIRFOIL-006, REQ-IND.AIRFOIL-009]
covers: [GOM.SPE.05, PCD.IND.01, PW.IND.01]
depends_on: [IND.AIRFOIL.SECTION, KRN]
cli: [fid pack evaluate-airfoil]
mcp: [pack_evaluate_airfoil]
journal: [pack.evaluate_airfoil@1]
updated: 2026-11-16
---

# IND.AIRFOIL.PARAMS: Airfoil section parameters

## Purpose

Computes the parameters customers tolerance on a blade section, for both the measured and the nominal curve of an `AirfoilSection`: mean camber line, chord, maximum thickness and its position, thickness at declared chord fractions, leading and trailing edge radius and thickness, stagger angle and section centroid. Emits each toleranced parameter as a ResultRecord.

## Scope

**In:**
- Mean camber line construction (method selected and versioned)
- Chord length and chord line
- Maximum thickness, its chordwise position, and thickness at declared chord fractions
- Leading and trailing edge radius (one or more explicitly selected definitions) and edge thickness at declared offsets
- Stagger angle against a declared reference axis in the section plane
- Section area centroid
- ResultRecord emission with UEC-0 unless an uncertainty evaluation is attached
- `pack.evaluate_airfoil@1` (R1)

**Out** (and which node owns it instead):
- Zone split and section validity (owned by `IND.AIRFOIL.SECTION`)
- Zone form deviation (owned by `IND.AIRFOIL.FORM`)
- Multi-section bow, lean and twist (owned by `IND.AIRFOIL.STACK`)
- Circle and spline fits (owned by `KRN`)

## Requirements

- **REQ-IND.AIRFOIL.PARAMS-001** When `pack.evaluate_airfoil@1` runs on a valid section, the pack shall compute chord length, maximum thickness, maximum-thickness chordwise position, thickness at each declared chord fraction, stagger angle and area centroid for both measured and nominal curves.
- **REQ-IND.AIRFOIL.PARAMS-002** The pack shall construct the mean camber line by an explicitly selected, versioned method (at least the locus of centres of maximum inscribed circles [unverified as industry default]).
- **REQ-IND.AIRFOIL.PARAMS-003** The pack shall compute leading and trailing edge radius by an explicitly selected, versioned definition (at least a least-squares circle over the edge zone) and shall record the definition's method ID in each ResultRecord.
- **REQ-IND.AIRFOIL.PARAMS-004** The pack shall compute leading and trailing edge thickness at each declared offset from the edge extreme point along the camber line.
- **REQ-IND.AIRFOIL.PARAMS-005** Every parameter compared with a tolerance shall be emitted as a ResultRecord through `IND.CONTRACT` with UEC-0 unless an uncertainty evaluation is attached, and shall never carry a fabricated uncertainty.
- **REQ-IND.AIRFOIL.PARAMS-006** If a parameter depends on a zone marked `NOT_DECIDABLE`, then the pack shall report that parameter as `NOT_DECIDABLE` rather than estimating it.
- **REQ-IND.AIRFOIL.PARAMS-007** The pack shall evaluate a section of up to 100 000 points in under 1 s on the reference hardware profile [unverified].

## Interfaces

- **Offers:** `pack.evaluate_airfoil@1` (R1) returning `AirfoilParams` for measured and nominal plus ResultRecords for toleranced parameters; camber-line and chord API for FORM and STACK.
- **Consumes:** `IND.AIRFOIL.SECTION` zones; KRN 2D circle fits, curve sampling and medial-axis primitives; `IND.CONTRACT.PROV` stamping.

## Decomposition hints

CAMBER (camber line and chord), THICK (thicknesses), EDGE (edge radius and thickness definitions), POSE (stagger and centroid), EMIT (ResultRecord emission).

## Open questions

- Several leading-edge radius definitions exist in industry practice (best-fit circle over a zone, curvature at the stagnation point) [unverified]. The list to ship needs the pack maintainer.
- Chord definition (extreme-point distance versus projection on a tangent line) varies by customer [unverified]; it should be a selectable method as well.

## Children

_None yet: status `open`._
