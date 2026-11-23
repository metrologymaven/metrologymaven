---
id: IND.TUBE
title: Pipe and tube pack
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
covers: [PW.IND.04, PCD.IND.04]
depends_on: [IND.CONTRACT, KRN, SCAN, ALN]
cli: [fid pack *-tube*]
mcp: [pack_*_tube*]
journal: [pack.*_tube*]
updated: 2026-11-22
---

# IND.TUBE: Pipe and tube pack

## Purpose

> **Commercial boundary (D-053):** Implemented commercially against the public `IND.CONTRACT` extension point in `metrologymaven/metrologymaven-enterprise`. Do not expand or build `IND.TUBE.*` in this public repository.

Inspection of bent tubes and pipes: reconstruct the measured centreline from scan or tactile data, express it as bend data, compare it with nominal, and produce bend corrections for the bending machine operator. Tube shops expect this as a dedicated workflow, so it ships as a commercial pack built strictly on `IND.CONTRACT`.

## Scope

**In:**
- Tube element: nominal centreline as straights and bends, outside diameter, bend radius per bend, end conditions
- Measured centreline reconstruction from cylinder segments (scan or tactile)
- Bend data representations: XYZ intersection points and length-rotation-angle (LRA, also called YBC)
- Comparison of measured versus nominal: per-straight length, per-bend rotation and angle, end-point and end-direction deviation, envelope check
- Best-fit of measured tube to nominal under declared constraints (ends fixed or free)
- Bend correction values for the next part
- Tube report template and bend-data export through registered exporters

**Out** (and which node owns it instead):
- Cylinder fitting (owned by `KRN`)
- Generic alignments (owned by `ALN`)
- Live communication with bending machines (owned by `HAL`; not yet catalogued)
- Wall thickness and ovality from scan data (owned by `SCAN`)
- Pack manifest, verification harness and provenance (owned by `IND.CONTRACT`)

## Requirements

- **REQ-IND.TUBE-001** The tube pack shall register its commands, element types, checks, exporters and report templates only through the `IND.CONTRACT` manifest.
- **REQ-IND.TUBE-002** When given fitted cylinder segments for every straight of a tube, the tube pack shall reconstruct the measured centreline as XYZ intersection points and as LRA bend data.
- **REQ-IND.TUBE-003** The tube pack shall convert between XYZ and LRA representations such that a round trip reproduces the input within 1e-12 m and 1e-12 rad [unverified].
- **REQ-IND.TUBE-004** When measured and nominal tubes are compared, the tube pack shall report per-straight length deviation, per-bend rotation and angle deviation, and end-point and end-direction deviation under a declared best-fit constraint.
- **REQ-IND.TUBE-005** If two adjacent straights are collinear within the declared angle threshold or a straight is shorter than the declared minimum, then the tube pack shall refuse the reconstruction with a typed error naming the straight.
- **REQ-IND.TUBE-006** When asked for corrections, the tube pack shall compute per-bend LRA corrections that would null the measured deviation, as a derived result marked advisory and never as a value of record for conformance.
- **REQ-IND.TUBE-007** The tube pack manifest shall declare its methods as versioned method IDs and shall name a maintainer.
- **REQ-IND.TUBE-008** The tube pack shall ship reference datasets including a synthetic multi-bend tube with known injected deviations, matched within 1e-8 m and 1e-8 rad [unverified].

## Interfaces

- **Offers:** element type `Tube`; commands such as `pack.evaluate_tube@1` (R1), `pack.create_tube_check@1` (W2), `pack.export_tube_benddata@1` (writes to an approved export root); tube report template.
- **Consumes:** KRN cylinder fits; SCAN point data; ALN best-fit solver; IND.CONTRACT manifest and provenance.

## Decomposition hints

MODEL (tube element and nominal), RECON (centreline reconstruction), BEND (XYZ and LRA conversion), CMP (comparison and best-fit), CORR (bend corrections), EXP (bend-data export), DATA, TPL.

## Split rationale

The split follows the **tube-shop workflow**: define the nominal (MODEL), reconstruct the measured centreline from cylinder fits (RECON), convert between XYZ and LRA (BEND), compare (CMP), correct (CORR), export bend data (EXP), plus datasets (DATA) and template (TPL). BEND is separate from RECON because the conversion is used by MODEL, CMP, CORR and EXP alike and is pure geometry that can be finished as a leaf now. Corrections are separate from comparison because they are advisory and never values of record (REQ-IND.TUBE-006).

Alternatives considered:
- **Merge RECON and BEND.** Rejected: nominals need conversion without reconstruction, and keeping them apart gives two clean leaves.
- **Merge CORR into EXP.** Rejected: corrections are also shown in reports, and the correction model needs its own owner.

## Open questions

- Which bender file formats to export first needs a tube-shop expert; formats must be implemented from public specifications only (CONSTITUTION §8 clean room).
- Live bender communication is not in `seeds/devices.md`; if wanted, HAL needs a device row.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IND.TUBE.MODEL | Tube element and nominal (leaf) | 001 | PW.IND.04, PCD.IND.04 |
| IND.TUBE.RECON | Measured centreline reconstruction (leaf) | 002 005 | PW.IND.04, PCD.IND.04 |
| IND.TUBE.BEND | XYZ and LRA bend data conversion (leaf) | 002 003 | PW.IND.04, PCD.IND.04 |
| IND.TUBE.CMP | Measured versus nominal tube comparison | 004 | PW.IND.04, PCD.IND.04 |
| IND.TUBE.CORR | Bend corrections | 006 | PW.IND.04, PCD.IND.04 |
| IND.TUBE.EXP | Bend-data export | 001 | PW.IND.04, PCD.IND.04 |
| IND.TUBE.DATA | Tube method declarations and reference datasets | 007 008 | none |
| IND.TUBE.TPL | Tube report template | 001 | none |
