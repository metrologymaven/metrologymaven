---
id: IND.AIRFOIL
title: Airfoil and blade pack
level: 2
parent: IND
kind: subsystem
status: split
commercial_repo: metrologymaven-enterprise
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND-001, REQ-IND-002, REQ-IND-003, REQ-IND-004]
covers: [GOM.SPE.05, PCD.IND.01, PW.IND.01]
depends_on: [IND.CONTRACT, KRN, GDT, SCAN]
cli: [fid pack *-airfoil*]
mcp: [pack_*_airfoil*]
journal: [pack.*_airfoil*]
updated: 2026-11-22
---

# IND.AIRFOIL: Airfoil and blade pack

## Purpose

> **Commercial boundary (D-053):** Implemented commercially against the public `IND.CONTRACT` extension point in `metrologymaven/metrologymaven-enterprise`. Do not expand or build `IND.AIRFOIL.*` in this public repository.

Gauging of turbine and compressor blades and other airfoil sections: from measured 2D section curves (scan sections or tactile scans) and their nominals, derive the airfoil parameters customers tolerance (chord, thicknesses, edge radii, camber, angles, section position and twist) and evaluate them. Shipped as a commercial pack built strictly on `IND.CONTRACT`.

## Scope

**In:**
- Airfoil section element: a closed 2D section curve with nominal, split into pressure side, suction side, leading edge and trailing edge zones
- Mean camber line construction
- Parameters: chord length, maximum thickness and its position, thickness at stated chord fractions, leading and trailing edge radius and thickness, stagger (twist) angle, section centroid position (bow and lean across a stack of sections)
- Zone-wise profile form with best-fit options restricted to the section plane (translation and rotation allowances declared per evaluation)
- Stacking of several sections along the blade span
- Report template for blade sections

**Out** (and which node owns it instead):
- Producing section curves from meshes or point clouds (owned by `SCAN`)
- Tactile scanning paths on a CMM (owned by `CMM`)
- Generic profile-of-a-line tolerance (owned by `GDT`)
- 2D fitting primitives such as circles and splines (owned by `KRN`)
- Pack manifest, verification harness and provenance (owned by `IND.CONTRACT`)

## Requirements

- **REQ-IND.AIRFOIL-001** The airfoil pack shall register its commands, element types, checks and report templates only through the `IND.CONTRACT` manifest.
- **REQ-IND.AIRFOIL-002** When given a closed measured section curve and its nominal, the airfoil pack shall split the section into pressure side, suction side, leading edge and trailing edge zones using a zone definition declared in the evaluation, and shall journal that definition.
- **REQ-IND.AIRFOIL-003** The airfoil pack shall compute chord length, maximum thickness, maximum-thickness position, leading and trailing edge radius, leading and trailing edge thickness at declared offsets, stagger angle and section centroid for both measured and nominal sections.
- **REQ-IND.AIRFOIL-004** If a section curve is open, self-intersecting or has a gap larger than the declared closure tolerance, then the airfoil pack shall refuse the evaluation with a typed error and shall not fabricate parameters.
- **REQ-IND.AIRFOIL-005** When several sections of one blade are evaluated together, the airfoil pack shall report per-section centroid offset and twist relative to a declared stacking axis.
- **REQ-IND.AIRFOIL-006** Every airfoil parameter compared with a tolerance shall be emitted as a ResultRecord through `IND.CONTRACT`, with UEC-0 unless an uncertainty evaluation is attached.
- **REQ-IND.AIRFOIL-007** The airfoil pack manifest shall declare its parameter definitions as versioned method IDs (no international standard is assumed to define them [unverified]) and shall name a maintainer.
- **REQ-IND.AIRFOIL-008** The airfoil pack shall ship at least three reference datasets (a symmetric analytic profile, a cambered analytic profile and a measured blade section), and on the analytic profiles every parameter shall match the analytic value within 1e-9 m for lengths and 1e-9 rad for angles [unverified].
- **REQ-IND.AIRFOIL-009** The airfoil pack shall evaluate a section of up to 100 000 points in under 1 s on the reference hardware profile [unverified].

## Interfaces

- **Offers:** element type `AirfoilSection`; commands such as `pack.evaluate_airfoil@1` (R1) and `pack.create_airfoil_check@1` (W2); report template for blade sections.
- **Consumes:** SCAN section curves; KRN 2D fits and curve operations; GDT profile-of-a-line evaluation for zone form; IND.CONTRACT manifest and provenance.

## Decomposition hints

SECTION (element and zone split), CAMBER (mean camber line), PARAMS (chord, thickness, edges, angles), STACK (multi-section bow, lean, twist), DATA (reference datasets), TPL (report template).

## Split rationale

The split follows the **evaluation pipeline of a blade section**: the element and its zones (SECTION), single-section parameters (PARAMS), zone form (FORM), multi-section stacking (STACK), then the pack's declarations and datasets (DATA) and its report template (TPL). Camber line is kept inside PARAMS rather than its own child because chord, thicknesses and edge definitions all depend on it and nothing else does.

Alternatives considered:
- **One child per parameter** (chord, thickness, edges, angles). Rejected: they share the camber line and zone data, so children would depend on each other in a chain.
- **Merge FORM into PARAMS.** Rejected: form is a deviation evaluation that needs ALN and GDT; parameters are geometric constructions that need only KRN.

## Open questions

- Several leading-edge radius definitions exist in industry practice (best-fit circle over a zone, curvature at the stagnation point) [unverified]. The pack should support more than one, selected explicitly; a domain maintainer should confirm the list. Carried by `IND.AIRFOIL.PARAMS`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IND.AIRFOIL.SECTION | Airfoil section element and zone split | 001 002 004 | GOM.SPE.05, PCD.IND.01, PW.IND.01 |
| IND.AIRFOIL.PARAMS | Airfoil section parameters | 003 006 009 | GOM.SPE.05, PCD.IND.01, PW.IND.01 |
| IND.AIRFOIL.FORM | Zone-wise airfoil profile form | 001 006 | GOM.SPE.05, PCD.IND.01, PW.IND.01 |
| IND.AIRFOIL.STACK | Blade section stacking | 005 006 | GOM.SPE.05, PCD.IND.01, PW.IND.01 |
| IND.AIRFOIL.DATA | Airfoil method declarations and reference datasets | 007 008 | none |
| IND.AIRFOIL.TPL | Blade section report template | 001 | none |
