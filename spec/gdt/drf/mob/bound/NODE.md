---
id: GDT.DRF.MOB.BOUND
title: Envelope constraints from MMB, LMB and rocking
level: 4
parent: GDT.DRF.MOB
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF.MOB-001, REQ-GDT.DRF.MOB-004]
covers: [PW.GDT.07]
depends_on: [GDT.DRF.MOB.REPR, GDT.DRF.SIM, GDT.RES.RESOLVE, KRN.ENV]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.DRF]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.MOB.BOUND: Envelope constraints from MMB, LMB and rocking

## Purpose

Derives the constraints of a mobility envelope from what the datum feature and the resolved DMT row say: the material boundary size (MMB or LMB) and its location and orientation relative to higher-precedence datums, the actual datum feature geometry (for example its maximum inscribed or minimum circumscribed envelope), and, where the row says so, the rocking freedom of a primary datum. Which boundary applies, how its size is fixed and whether rocking contributes mobility are interpretation content of signed-off DMT rows (D-009, D-016); this node turns those choices into `MobilityConstraint` values.

## Scope

**In:**
- Boundary size and placement from the resolved row for planar, cylindrical and width datum features
- Constraint generation in the `GDT.DRF.MOB.REPR` vocabulary
- Rocking-derived constraints where the row calls for them
- Matching every ORS-DRF mobility case

**Out** (and which node owns it instead):
- Envelope type and digest (owned by `GDT.DRF.MOB.REPR`)
- Rigid groups (owned by `GDT.DRF.MOB.GROUP`)
- Rocking candidate enumeration (owned by `GDT.DRF.SIM.ROCK`)
- Boundary rules as content (owned by DMT rows in `GDT.RES`)

## Requirements

- **REQ-GDT.DRF.MOB.BOUND-001** When a datum feature is referenced at MMB or LMB, the system shall build the envelope constraints from the boundary size and placement the resolved row defines and the actual datum feature geometry, and record the row ID in the envelope.
- **REQ-GDT.DRF.MOB.BOUND-002** If the row's boundary needs a constraint kind `GDT.DRF.MOB.REPR` cannot represent, then the system shall return `UNSUPPORTED{reason: constraint_kind}` and build no envelope, never a silent approximation.
- **REQ-GDT.DRF.MOB.BOUND-003** Envelopes shall match every ORS-DRF mobility case within its stated tolerance.

## Interfaces

- **Offers:** `build_envelope(drf, datum_features, chain) -> MobilityEnvelope` used by `GDT.ZONE.MOBSRCH` and `GDT.DRF.CHECK.MEMB`.
- **Consumes:** envelope type (GDT.DRF.MOB.REPR); simulated datums and candidates (GDT.DRF.SIM); envelope fits (KRN.ENV); DMT rows (`GDT.RES.ROWASME`, `GDT.RES.ROWISO`, read as data); ORS-DRF cases (`UNC.ORS.DRF`, read as data).

## Decomposition hints

PLANE (planar and width datums at MMB or LMB), CYL (cylindrical datums), ROCK (rocking-derived freedom), each a leaf once signed-off rows define the boundary.

## Open questions

- Not a leaf: boundary definitions (sizes, placement relative to higher datums, rocking contribution) are DMT content (D-009; texts per D-016).
- Inherited from `GDT.DRF.MOB`: whether non-convex envelopes (rocking plus clearance) must be supported or approximated conservatively with a flag; an expert call through the DMT. REQ-002 forbids silent approximation either way.
- Inherited from `GDT.DRF.COMMON`: how a common datum's mobility is represented; expected to be one envelope over the common datum's simulated axis or plane, confirmed when `GDT.DRF.COMMON.CDAT` rows exist.

## Children

_None yet: status `open`._
