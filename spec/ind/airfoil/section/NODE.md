---
id: IND.AIRFOIL.SECTION
title: Airfoil section element and zone split
level: 3
parent: IND.AIRFOIL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.AIRFOIL-001, REQ-IND.AIRFOIL-002, REQ-IND.AIRFOIL-004]
covers: [GOM.SPE.05, PCD.IND.01, PW.IND.01]
depends_on: [IND.CONTRACT, SCAN, KRN]
cli: [fid pack create-airfoil-section]
mcp: [pack_create_airfoil_section]
journal: [pack.create_airfoil_section@1]
updated: 2026-11-16
---

# IND.AIRFOIL.SECTION: Airfoil section element and zone split

## Purpose

The `AirfoilSection` element: a closed measured 2D section curve in a declared section plane, paired with its nominal, validated for closure, and split into pressure side, suction side, leading edge and trailing edge zones by an explicit, journaled zone definition. Every other airfoil child works on this element.

## Scope

**In:**
- Element type `AirfoilSection` (section plane frame, measured curve, nominal curve, closure tolerance, zone definition), registered through the pack manifest
- Closure and validity checks (open, self-intersecting, gap above closure tolerance)
- Zone definitions: leading and trailing edge zones by chord fraction or by arc length from the extreme points, with pressure and suction sides as the remainder
- Transfer of zones from nominal to measured by closest-point correspondence
- `pack.create_airfoil_section@1` (W2; agents write to an agent branch)

**Out** (and which node owns it instead):
- Producing section curves from meshes or point clouds (owned by `SCAN`)
- Parameter computation (owned by `IND.AIRFOIL.PARAMS`)
- Zone form evaluation (owned by `IND.AIRFOIL.FORM`)

## Requirements

- **REQ-IND.AIRFOIL.SECTION-001** The `AirfoilSection` element type and its create command shall be registered only through the airfoil pack manifest.
- **REQ-IND.AIRFOIL.SECTION-002** When a section is created, the pack shall store the section-plane frame, measured curve, nominal curve, closure tolerance and zone definition, and shall journal the zone definition with the command.
- **REQ-IND.AIRFOIL.SECTION-003** The zone definition shall be one of a versioned set of methods (at least: edge zones by chord fraction; edge zones by arc length from the leading and trailing extreme points), selected explicitly with its parameters.
- **REQ-IND.AIRFOIL.SECTION-004** If the measured or nominal curve is open, self-intersecting or has a gap larger than the declared closure tolerance, then the pack shall refuse to create the section with `E_SECTION_OPEN`, `E_SECTION_SELF_INTERSECT` or `E_SECTION_GAP` and shall journal nothing.
- **REQ-IND.AIRFOIL.SECTION-005** When zones are transferred from nominal to measured, each measured point shall take the zone of its closest nominal point, and the pack shall report the number of points per zone.
- **REQ-IND.AIRFOIL.SECTION-006** If any zone of the measured section receives fewer points than the declared minimum, then the pack shall mark characteristics that depend on that zone as `NOT_DECIDABLE`.

## Interfaces

- **Offers:** element type `AirfoilSection`; `pack.create_airfoil_section@1` (W2); zone labelling API for PARAMS, FORM and STACK.
- **Consumes:** SCAN section curves; KRN 2D curve operations (closest point, self-intersection test); `IND.CONTRACT` manifest.

## Decomposition hints

ELEM (element and create command), ZONE (zone definitions and transfer). ZONE may become a leaf quickly.

## Open questions

- Which zone definitions turbomachinery users expect first needs the pack maintainer [unverified].

## Children

_None yet: status `open`._
