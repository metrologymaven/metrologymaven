---
id: KRN.ENV
title: Envelope, tangential and constrained fits
level: 2
parent: KRN
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN-002, REQ-KRN-005, REQ-KRN-007]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.LSQ, KRN.SOLV, KRN.MAN]
cli: [fid fit mcc-*, fid fit mic-*, fid fit tan-*]
mcp: [fit_mcc_*, fit_mic_*, fit_tan_*]
journal: [fit.mcc_*, fit.mic_*, fit.tan_*]
updated: 2026-11-16
---

# KRN.ENV: Envelope, tangential and constrained fits

## Purpose

Fits that sit on one side of the material: minimum circumscribed (MCC), maximum inscribed (MIC), and inner and outer tangential fits, including the material-side datum simulators. They drive mating envelopes, ASME Rule #1 and MMC bonus in GDT. Several are non-convex and fail in characteristic ways on sparse or partial data, so they need their own safeguards and evidence.

## Scope

**In:**
- Minimum circumscribed fits for 2D circle, sphere and cylinder; maximum inscribed fits for 2D circle, sphere and cylinder; and the corresponding envelope fits for slot and rectangle where defined
- Inner and outer tangential fits for line, plane and cylinder: external minimax, constrained L2 and L1 outer-tangent variants
- Candidate-datum-set handling for rocking features (report every stable contact set, not one arbitrary choice)
- Convex-case cross-checks (for example, a seeded Welzl-style algorithm for MCC circle and sphere)
- Exact 2D Voronoi path for MIC circle as a cross-check
- Centre and axis trust region tied to the least-squares solution, so a sparse or partial set cannot let the centre escape
- Spec cards and algorithm IDs `fit.<primitive>.mcc|mic|tan_*@<semver>`

**Out** (and which node owns it instead):
- The `Fit` trait, least-squares starts and fixed-parameter LS constraints (`KRN.LSQ`)
- Minimum-zone fits (`KRN.MZ`)
- Conic solvers and certificates (`KRN.SOLV`)
- Choosing which fit a datum or size uses under a dialect, Rule #1, bonus and datum mobility (`GDT`)

## Requirements

- **REQ-KRN.ENV-001** The subsystem shall provide minimum circumscribed fits for 2D circle, sphere and cylinder, and maximum inscribed fits for 2D circle, sphere and cylinder, each implementing the `Fit` trait.
- **REQ-KRN.ENV-002** The subsystem shall provide inner and outer tangential fits for line, plane and cylinder in the external-minimax, constrained-L2 and constrained-L1 variants, selected by an explicit parameter.
- **REQ-KRN.ENV-003** If a primitive and method pair is not defined (for example, maximum inscribed for an open line), then the subsystem shall return a typed `method_undefined` error.
- **REQ-KRN.ENV-004** For convex cases (MCC circle and sphere), the result shall carry a duality gap below 1e-12 m [unverified] and shall match an independent combinatorial algorithm within 1e-9 m [unverified].
- **REQ-KRN.ENV-005** For non-convex cases (all MIC, and MCC cylinder), the result shall carry the contact set, a local optimality check and the `local_optimum_only` flag whenever global optimality is not proven.
- **REQ-KRN.ENV-006** The MIC fits shall constrain the centre or axis to a trust region around the least-squares solution, and if the optimum lies on the trust-region boundary, then the fit shall return a typed `unbounded_envelope` error.
- **REQ-KRN.ENV-007** When a tangential fit has more than one stable contact set (a rocking feature), the result shall list every candidate contact set found and shall mark which one the deterministic tie-break selected.
- **REQ-KRN.ENV-008** For the 2D MIC circle, the exact Voronoi path and the conic path shall agree within 1e-9 m [unverified] on every reference case, and any disagreement shall fail CI.
- **REQ-KRN.ENV-009** When an envelope or tangential fit command is committed, the system shall journal one `fit.mcc_*`, `fit.mic_*` or `fit.tan_*` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.mcc_<primitive>@1`, `fit.mic_<primitive>@1`, `fit.tan_<primitive>@1`; matching CLI, MCP and journal names; envelope results with `local_optimum_only` for `GDT` (mating envelopes, Rule #1, bonus, datum simulators).
- **Consumes:** `KRN.LSQ`, `KRN.SOLV`, `KRN.MAN`.

## Decomposition hints

MCC (circumscribed and convex cross-check), MIC (inscribed with trust region and Voronoi check), TAN (tangential and datum-simulator variants, rocking candidates).

## Open questions

- No NMI reference datasets exist for MIC or MCC on partial features [unverified]; validation relies on the open reference suites in `UNC`.
- The planar datum default (minimax versus constrained L2) is an edition-specific choice that belongs to `GDT`; this subsystem only provides both.

## Split rationale

The split follows the **association criterion**, because each has its own objective, failure mode and safeguard: minimum circumscribed (MCC: convex for circle and sphere, non-convex for cylinder), maximum inscribed (MIC: always non-convex, needs the trust region), and tangential and datum-simulator fits (TAN: one-sided contact, rocking candidate sets). The independent combinatorial cross-checks (Welzl for MCC, Voronoi for MIC) form a fourth child, XCHK, so the paths that check the conic results are built and reviewed apart from them; XCHK reuses the Voronoi primitives of `KRN.MZ.VOR`.

Alternatives considered:
- **Split by primitive (circle, sphere, cylinder, plane).** Rejected: the safeguards (trust region, rocking sets, convex cross-check) are per criterion, not per primitive.
- **Cross-checks inside MCC and MIC.** Rejected for the same independence reason as `KRN.MZ.VOR`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.ENV.MCC | Minimum circumscribed fits | 001 003 004 005 009 | GOM.CON.20, PW.CON.05 |
| KRN.ENV.MIC | Maximum inscribed fits with trust region | 001 003 005 006 008 009 | GOM.CON.20, PW.CON.05 |
| KRN.ENV.TAN | Tangential fits and datum simulators | 002 003 005 007 009 | GOM.CON.20, PW.CON.05 |
| KRN.ENV.XCHK | Combinatorial cross-checks for MCC and MIC | 004 008 | none |
