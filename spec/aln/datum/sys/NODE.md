---
id: ALN.DATUM.SYS
title: Datum-system alignment engine and ASME dialect
level: 3
parent: ALN.DATUM
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM-003, REQ-ALN.DATUM-004, REQ-ALN.DATUM-005, REQ-ALN.DATUM-006, REQ-ALN.DATUM-007]
covers: [GOM.ALN.07, PCD.ALN.05, PW.ALN.06]
depends_on: [ALN.MODEL.ELEM, ALN.QA.REPORT, GDT.RES, GDT.DRF]
cli: [fid align run-datum-system]
mcp: [align_run_datum_system]
journal: [align.run_datum_system@1]
updated: 2026-11-17
---

# ALN.DATUM.SYS: Datum-system alignment engine and ASME dialect

## Purpose

Aligns the part to a datum reference frame named by datum features in precedence (primary, secondary, tertiary) under an explicitly stated dialect, edition and defaults profile. It obtains the datum-feature simulators and constraint order from GDT's interpretation resolver, builds the frame (delegating DRF construction to `GDT.DRF` where available), wraps the result as an `Alignment` with the operator-chain digest, and serves as the starting frame GDT then evaluates with mobility. P1a enables ASME Y14.5-2018 rows; ISO 5459:2011 is enabled by `ALN.DATUM.ISO`.

## Scope

**In:**
- The dialect gate: dialect, standard edition and defaults profile must be stated
- Resolver call per datum feature and recording of the DMT row IDs, chain digest and DMT digest
- Precedence application through GDT.DRF's construction and DoF table (GDT.DRF.BUILD)
- The ASME Y14.5-2018 path (RMB datum features in P1a, matching `GDT.DRF`'s P1a scope)
- `align.run_datum_system@1` and alignment wrapping

**Out** (and which node owns it instead):
- DMT rows and the resolver (`GDT.RES`); DRF construction, rocking-datum candidate sets and mobility (`GDT.DRF`)
- ISO 5459 enablement (`ALN.DATUM.ISO`)
- Element alignment without a dialect (`ALN.DATUM.ELEM`)
- Datum-feature simulator fits (`KRN.ENV`)

## Requirements

- **REQ-ALN.DATUM.SYS-001** If `align.run_datum_system` is called without an explicit dialect, standard edition and defaults profile, then the command shall fail with `dialect_unspecified` and journal nothing.
- **REQ-ALN.DATUM.SYS-002** For each datum feature, the system shall obtain the simulator and constraint order from GDT's resolver, and if the resolver returns `UNRESOLVED` or `UNSUPPORTED`, then the command shall fail with that outcome and the DMT key, and journal nothing.
- **REQ-ALN.DATUM.SYS-003** The system shall apply datum features in their stated precedence so that a lower-precedence datum never changes a DoF already constrained by a higher-precedence datum by more than 1e-12 m or 1e-12 rad.
- **REQ-ALN.DATUM.SYS-004** The alignment shall record the dialect, edition, defaults profile, each DMT row ID, the operator-chain digest and the DMT digest.
- **REQ-ALN.DATUM.SYS-005** Given the published ASME datum-alignment fixtures with a known transform, the recovered frame shall match within 1e-9 m and 1e-9 rad for noise-free data [unverified].
- **REQ-ALN.DATUM.SYS-006** When the datum features leave DoF unconstrained, the system shall report them and apply the user's `keep_parent` or `reject` policy through `ALN.DATUM.DOF`.
- **REQ-ALN.DATUM.SYS-007** The computation shall run in the CPU kernel under KNP-1 and declare its determinism class in the registry (D0 for closed-form and LSQ simulators, D3 when a simulator is an iterative envelope fit).

## Interfaces

- **Offers:** `align.run_datum_system@1` (W2; params `datums: [primary, secondary?, tertiary?]` as datum-feature references, `dialect`, `edition`, `defaults_profile`, `unconstrained`, `parent`); the resulting alignment as GDT's starting frame.
- **Consumes:** `GDT.RES` resolver; `GDT.DRF.SIM` simulators and `GDT.DRF.BUILD` DRF construction with its DoF table; `ALN.MODEL.ELEM`; `ALN.QA.REPORT`.

## Decomposition hints

GATE (dialect gate and resolver calls), BUILD (frame build via GDT.DRF or level solve), RECORD (chain digests and alignment wrapping), FIXT (ASME datum-alignment fixtures).

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** ALN may depend on `fid-gps` (now L0), so the dependency on GDT.RES/GDT.DRF pure functions is layer-legal.
- **Resolved: overlap with GDT.DRF.** BUILD calls GDT.DRF's pure construction (the R1 path of `gdt.build_drf@1`, GDT.DRF.BUILD) and simulators (GDT.DRF.SIM); GDT.DRF owns the single DoF table, so this node no longer uses `ALN.DATUM.DOF` or `ALN.DATUM.ELEM` (removed from depends_on). The unconstrained policy (`keep_parent` / `reject`) keeps the same names and error code as ALN.DATUM.DOF.
- **Unresolved: layer order.** CONSTITUTION §2.1 puts ALN before GDT in L2, so ALN may not depend on GDT. GATE and BUILD depend on GDT.RES.RESOLVE, GDT.DRF.SIM and GDT.DRF.BUILD, which are pure kernel code in `fid-gps`. This is layer-legal only if `fid-gps` (resolver, DMT schema, DRF construction, simulators) is classed L0; otherwise datum-system alignment must move to GDT. Needs a ruling at review (possibly an amendment to D-034's L0 list); not opened as a decision because the caller fixed this boundary.
- The GDT/ALN dependency cycle risk noted in the parent remains: GDT.DRF depends on ALN.MODEL and ALN.BFIT, and this node depends on GDT.DRF. That is acyclic only while ALN.MODEL and ALN.BFIT never depend on ALN.DATUM.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.DATUM.SYS.GATE | Dialect gate and per-datum resolver calls (leaf) | 001 002 004 007 | none |
| ALN.DATUM.SYS.BUILD | Datum-system frame build through GDT.DRF and unconstrained-DoF policy (leaf) | 003 005 006 007 | none |
| ALN.DATUM.SYS.CMD | Datum-system alignment command and recording (leaf) | 001 002 004 006 007 | GOM.ALN.07, PCD.ALN.05, PW.ALN.06 |
| ALN.DATUM.SYS.FIXT | Datum-system alignment fixtures with known transforms (leaf) | 005 | none |
