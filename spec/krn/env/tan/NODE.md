---
id: KRN.ENV.TAN
title: Tangential fits and datum simulators
level: 3
parent: KRN.ENV
kind: module
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV-002, REQ-KRN.ENV-003, REQ-KRN.ENV-005, REQ-KRN.ENV-007, REQ-KRN.ENV-009]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.LSQ.LIN, KRN.LSQ.AXIS, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART]
cli: [fid fit tan-line, fid fit tan-plane, fid fit tan-cylinder]
mcp: [fit_tan_line, fit_tan_plane, fit_tan_cylinder]
journal: [fit.tan_line@1, fit.tan_plane@1, fit.tan_cylinder@1]
updated: 2026-11-17
---

# KRN.ENV.TAN: Tangential fits and datum simulators

## Purpose

Inner and outer tangential fits for line, plane and cylinder: the feature that touches the material from one side, as a datum simulator does. Three objective variants exist (external minimax, constrained L2 and constrained L1 outer-tangent), and the planar datum default differs by standard edition, so all three are provided and selected by an explicit parameter. Rocking features have more than one stable contact set; every candidate is reported.

## Scope

**In:**
- Tangential line, plane and cylinder fits, inner and outer side, in external-minimax, constrained-L2 and constrained-L1 variants
- Candidate contact-set enumeration for rocking features and the deterministic selection marker
- Constraint residuals in the certificate
- Registry commands and spec cards

**Out** (and which node owns it instead):
- Choosing the variant per dialect and edition, datum precedence and mobility (`GDT`)
- Least-squares fits with fixed parameters (`KRN.LSQ`)

## Requirements

- **REQ-KRN.ENV.TAN-001** The module shall provide inner and outer tangential fits for line, plane and cylinder in the external-minimax, constrained-L2 and constrained-L1 variants, selected by a required parameter with no default.
- **REQ-KRN.ENV.TAN-002** Each result shall leave every point on the material side within a recorded tolerance and shall carry the contact set and constraint residuals in its certificate.
- **REQ-KRN.ENV.TAN-003** When more than one stable contact set exists, the result shall list every candidate found and mark the one chosen by the deterministic tie-break.
- **REQ-KRN.ENV.TAN-004** If a variant and primitive pair is undefined, then the system shall return `method_undefined`.
- **REQ-KRN.ENV.TAN-005** When a command is committed, the system shall journal one `fit.tan_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.tan_line@1`, `fit.tan_plane@1`, `fit.tan_cylinder@1` (W2, dry-run R1); candidate contact sets for `GDT` datum simulation.
- **Consumes:** `KRN.LSQ`, `KRN.SOLV`.

## Decomposition hints

PLANE (plane, three variants; the main datum case), LINE (line), CYL (cylinder), ROCK (candidate contact-set enumeration and selection marker).

## Open questions

- The constrained-L1 variant's exact definition should follow published work on datum establishment [unverified]; it must not be taken from standards text. Each leaf states the objective in its own words and cites published work by author and year only.
- A tangential 3D line has no defined material side; `fit.tan_line` is 2D (in a given plane) and a 3D request returns `method_undefined` (table in `KRN.ENV.TAN.ROCK`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.ENV.TAN.ROCK | Rocking candidate sets, tie-break and tangential variant table | 003 004 | none |
| KRN.ENV.TAN.PLANE | Tangential plane in three variants | 001 002 005 | GOM.CON.20, PW.CON.05 |
| KRN.ENV.TAN.LINE | Tangential 2D line in three variants | 001 002 005 | GOM.CON.20, PW.CON.05 |
| KRN.ENV.TAN.CYL | Tangential cylinder, inner and outer | 001 002 005 | GOM.CON.20, PW.CON.05 |
