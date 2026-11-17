---
id: KRN.CONS.AUTO
title: Automatic construction from CAD with nominal-actual pairing
level: 3
parent: KRN.CONS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-007, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [GOM.CON.22]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT]
cli: [fid construct auto-from-cad, fid construct infer-feature-type]
mcp: [construct_auto_from_cad, construct_infer_feature_type]
journal: [construct.auto_from_cad@1, construct.infer_feature_type@1]
updated: 2026-11-17
---

# KRN.CONS.AUTO: Automatic construction from CAD with nominal-actual pairing

## Purpose

Creates one nominal element per eligible CAD face (plane, cylinder, cone, sphere, torus, and planar circles and slots from face boundaries) and pairs each with actual data by a documented, parameterised rule, so a user can turn a CAD model into a feature list in one step, as scan-inspection incumbents do. Under D-029 it also owns the shared, kernel-level inference of a feature type from a small set of measured points, which is the same "automatic construction" problem without CAD.

## Scope

**In:**
- Face eligibility rules per primitive type and the nominal element each produces, from recognised-face data handed to the kernel as plain data
- Nominal-actual pairing rules (distance and normal-angle gates, minimum point count, conflict resolution between neighbouring faces) as explicit parameters
- A per-face report: created, skipped (with reason), paired, unpaired
- Feature-type inference from points: fit every candidate primitive and rank the candidates by one deterministic rule (D-029)
- The registry commands and spec cards

**Out** (and which node owns it instead):
- Reading CAD, topology and recognising analytic shapes on faces (`GEO.CAD.NOMINAL`, `GEO.CAD.TOPO`, `IO.CAD`)
- Retrieving candidate actual points per face from large scans by spatial query (`SCAN`)
- Fitting the actual elements (`KRN.LSQ` and siblings)
- The UI and draft flows that use inference (`CMM.STRAT.LEARN`, `IND.PORT.RECOG`)

## Requirements

- **REQ-KRN.CONS.AUTO-001** When the user asks to construct elements from a set of CAD faces, the system shall create exactly one nominal element per eligible face and shall list every ineligible face with the reason.
- **REQ-KRN.CONS.AUTO-002** The pairing rule shall be a function of explicit parameters (distance gate, normal-angle gate, minimum point count, conflict rule), all recorded in the parameter digest.
- **REQ-KRN.CONS.AUTO-003** When the same CAD, data and parameters are given, the pairing shall be identical, including the resolution of points claimed by two faces.
- **REQ-KRN.CONS.AUTO-004** When the command is committed, the system shall journal one `construct.auto_from_cad@1` event with algorithm ID, input element IDs, parameter digest and output digest.
- **REQ-KRN.CONS.AUTO-005** When asked to infer the feature type of a point set, the system shall return every candidate primitive with its fit outcome in one ranked list ordered by a single documented deterministic rule, so that every app that consumes it gets the same answer for the same points (D-029).

## Interfaces

- **Offers:** `construct.auto_from_cad@1` (W2, dry-run R1) and the per-face report; `construct.infer_feature_type@1` (R1) and the `TypeRanking` result for `CMM.STRAT.LEARN` and `IND.PORT.RECOG`; the plain-data `NominalFaceView` type that the bus fills from `GEO.CAD.NOMINAL` recognition results.
- **Consumes:** `KRN.CONS.DECL` (construction contract), `KRN.LSQ.TRAIT` (`PointsView`, `FittedFeature`), the least-squares fits of `KRN.LSQ.LIN`, `KRN.LSQ.RND` and `KRN.LSQ.AXIS` (inference only).

## Decomposition hints

FACE (eligibility, nominal creation, command and report), PAIR (pairing function), INFER (feature-type inference).

## Open questions

- **Layering resolved (CONSTITUTION §2.1, D-034):** the kernel never reads CAD. The bus resolves face handles through `GEO.CAD.NOMINAL` and passes `NominalFaceView` rows (plain data defined here) to the kernel. `GEO.CAD.NOMINAL` may depend on this type (same layer); this node does not depend on GEO.
- **D-029 applied:** feature-type inference lives in `KRN.CONS.AUTO.INFER`. REQ-KRN.CONS.AUTO-005 was added for it; `KRN.CONS` has no parent requirement for inference yet, so its owner should add one and trace AUTO to it. Until then AUTO-005 traces only to D-029.
- Overlap with `GEO.CAD.NOMINAL` (`geo.extract_nominal@1`, GOM.CAD.03): GEO recognises faces and creates nominal elements; this node consumes the same recognition results (it never recognises again), pairs them with actual data and reports per face.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.CONS.AUTO.FACE | Face eligibility, nominal creation and auto-from-CAD command | 001 003 004 | GOM.CON.22 |
| KRN.CONS.AUTO.PAIR | Nominal-actual point pairing with gates and conflict rule | 002 003 | GOM.CON.22 |
| KRN.CONS.AUTO.INFER | Feature-type inference from points (D-029) | 005 | none |
