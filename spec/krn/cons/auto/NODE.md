---
id: KRN.CONS.AUTO
title: Automatic construction from CAD with nominal-actual pairing
level: 3
parent: KRN.CONS
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-007, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [GOM.CON.22]
depends_on: [KRN.CONS.DECL, KRN.CONS.ELEM, GEO.CAD]
cli: [fid construct auto-from-cad]
mcp: [construct_auto_from_cad]
journal: [construct.auto_from_cad@1]
updated: 2026-11-16
---

# KRN.CONS.AUTO: Automatic construction from CAD with nominal-actual pairing

## Purpose

Creates one nominal element per eligible CAD face (plane, cylinder, cone, sphere, torus, and planar circles and slots from face boundaries) and pairs each with actual data by a documented, parameterised rule, so a user can turn a CAD model into a feature list in one step, as scan-inspection incumbents do.

## Scope

**In:**
- Face eligibility rules per primitive type and the nominal element each produces
- Nominal-actual pairing rules (distance and normal-angle gates, minimum point count, conflict resolution between neighbouring faces) as explicit parameters
- A per-face report: created, skipped (with reason), paired, unpaired
- The registry command and spec card

**Out** (and which node owns it instead):
- Reading CAD and face classification (`GEO.CAD`, `IO.CAD`)
- Extracting actual points from scans per face (`SCAN`)
- Fitting the actual elements (`KRN.LSQ` and siblings)

## Requirements

- **REQ-KRN.CONS.AUTO-001** When the user asks to construct elements from a set of CAD faces, the system shall create exactly one nominal element per eligible face and shall list every ineligible face with the reason.
- **REQ-KRN.CONS.AUTO-002** The pairing rule shall be a function of explicit parameters (distance gate, normal-angle gate, minimum point count, conflict rule), all recorded in the parameter digest.
- **REQ-KRN.CONS.AUTO-003** When the same CAD, data and parameters are given, the pairing shall be identical, including the resolution of points claimed by two faces.
- **REQ-KRN.CONS.AUTO-004** When the command is committed, the system shall journal one `construct.auto_from_cad@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Interfaces

- **Offers:** `construct.auto_from_cad@1` (W2, dry-run R1) and the per-face report.
- **Consumes:** `KRN.CONS.DECL`, `KRN.CONS.ELEM`, face data from `GEO.CAD`.

## Decomposition hints

ELIG (face eligibility and nominal creation), PAIR (pairing rules and conflict resolution), RPT (per-face report).

## Open questions

- The same layering conflict as `KRN.CONS.DERIV`: CAD face data comes from `GEO.CAD`, above the kernel. The kernel part may be limited to the pairing function over plain data.

## Children

_None yet: status `open`._
