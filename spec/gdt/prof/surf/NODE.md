---
id: GDT.PROF.SURF
title: Profile of a surface, including composite profile
level: 3
parent: GDT.PROF
kind: module
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF-001, REQ-GDT.PROF-006]
covers: [GOM.GDT.12, PW.GDT.05]
depends_on: [GDT.PROF.DEV, GDT.PROF.ZDEF, GDT.ZONE.POSE, GDT.ZONE.COMP, GDT.PROF.BESTFIT]
cli: [fid check eval-surface-profile]
mcp: [check_eval_surface_profile]
journal: [check.eval_surface_profile@1]
updated: 2026-11-17
---

# GDT.PROF.SURF: Profile of a surface, including composite profile

## Purpose

Evaluates profile of a surface, the most general and most used freeform tolerance: all points of the toleranced surface must lie within the zone around the nominal surface, located and oriented by the DRF (location control), oriented only, or free (form control). Composite and multi-segment profile frames are solved per tier by `GDT.ZONE.COMP`; this module builds the profile problem for each tier and fills the payload.

## Scope

**In:**
- Profile of a surface with full DRF, partial DRF, or no datums
- Composite and multi-segment profile (tier problems handed to `GDT.ZONE.COMP`)
- Payload: profile value, signed minimum and maximum deviation, zone definition, pose used, approximation error carried from DEV

**Out** (and which node owns it instead):
- Best-fit options (owned by `GDT.PROF.BESTFIT`)
- Colour maps (owned by `SCAN` and `VIZ`)
- Tier solve (owned by `GDT.ZONE.COMP`)

## Requirements

- **REQ-GDT.PROF.SURF-001** The system shall evaluate profile of a surface from measured points and the nominal, and report the profile value, the signed minimum and maximum deviations, the zone definition and the pose used.
- **REQ-GDT.PROF.SURF-002** For a composite or multi-segment frame, the system shall report one value per tier from `GDT.ZONE.COMP`.
- **REQ-GDT.PROF.SURF-003** The ResultRecord shall carry the maximum nominal approximation error of the faces hit, so UNC can include it as a contributor.
- **REQ-GDT.PROF.SURF-004** Values shall match every ORS-GDT surface-profile and ORS-PROJ case within its stated tolerance.

## Interfaces

- **Offers:** `check.eval_surface_profile@1` (W2, dry-run R1).
- **Consumes:** DEV, ZDEF; zone engine (GDT.ZONE.POSE, GDT.ZONE.COMP); best-fit options (GDT.PROF.BESTFIT).

## Decomposition hints

FIXED (full DRF, no free DOF; a leaf once ZDEF rows exist), FREE (form control), COMP (composite tiers), CMD.

## Open questions

- Profile with "all around" and "all over" extents depends on face grouping from GEO. Resolved for whole faces: `GDT.PROF.SURF.PROB` takes the toleranced face set from the characteristic, either as face IDs or as a CAD group from `GEO.CAD.GROUP`. Extents that end part-way across a face ("between" points) need face trimming that no GEO node offers yet; they return `UNSUPPORTED{partial_face_extent}` until one does.

## Split rationale

Hints FIXED and FREE are merged into PROB: both build the same single-segment problem and differ only in which DOFs the DRF leaves free, which `GDT.ZONE.POSE` already handles (no free DOF means no solver call). COMP builds tier problems; CMD owns the command and payload.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.PROF.SURF.PROB | Single-segment surface-profile problem (leaf) | 001 004 | GOM.GDT.12 |
| GDT.PROF.SURF.COMP | Composite and multi-segment profile tiers (leaf) | 002 004 | PW.GDT.05 |
| GDT.PROF.SURF.CMD | Surface-profile command and payload (leaf) | 001 002 003 004 | GOM.GDT.12, PW.GDT.05 |
