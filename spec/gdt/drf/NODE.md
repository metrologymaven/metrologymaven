---
id: GDT.DRF
title: Datum reference frames and datum mobility
level: 2
parent: GDT
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT-005]
covers: [GOM.GDT.14, PW.GDT.07, SA.GDT.03]
depends_on: [GDT.CHAR, GDT.RES, KRN, ALN.MODEL, ALN.BFIT]
cli: [fid gdt *-drf, fid gdt *-datum]
mcp: [gdt_*_drf, gdt_*_datum]
journal: [gdt.*_drf, gdt.*_datum]
updated: 2026-11-16
---

# GDT.DRF: Datum reference frames and datum mobility

## Purpose

Builds the per-characteristic **datum reference frame (DRF)** from datum features in precedence order, using the datum-feature simulators the DMT row names (tangent / constrained fits from KRN), keeps book of which degrees of freedom each datum constrains, and computes the remaining **datum mobility** (from datum features referenced at MMB or LMB, and from rocking primary datums). The DRF is a distinct type from an alignment: alignments bring data into the part frame (ALN); a DRF is the reference for one tolerance and may move within its mobility.

## Scope

**In:**
- Datum feature and datum target model references (data lives in `GDT.CHAR`)
- DRF construction in precedence order (primary, secondary, tertiary), common datums (A-B) and datum targets
- Datum-feature simulators per DMT row: constrained L2, minimax and L1 tangent plane / line / cylinder, candidate datum sets for rocking features
- Degree-of-freedom bookkeeping by invariance class (ISO 17450-1)
- Datum mobility from datum features at MMB / LMB and from rocking: the mobility envelope as a set of allowed transforms
- DRF and mobility result fields: transform used, mobility envelope reference, whether the used transform is inside the envelope
- P1a: planar and cylindrical datums, RMB only; P1b: mobility (MMB / LMB), ISO 5459 rows

**Out** (and which node owns it instead):
- Part alignment (3-2-1, RPS, best-fit) (owned by `ALN`)
- The fits themselves (owned by `KRN`)
- Using the mobility envelope inside a tolerance-zone solve (owned by `GDT.ZONE`)
- Uncertainty of the DRF (owned by `UNC`)

## Requirements

- **REQ-GDT.DRF-001** The system shall construct a DRF from one to three datum features in the precedence order stated by the feature control frame, using the datum-feature simulator named by the resolved DMT row.
- **REQ-GDT.DRF-002** Each DRF shall record, per datum, which of the six degrees of freedom it constrains, and the DRF shall report the degrees of freedom that remain unconstrained.
- **REQ-GDT.DRF-003** When a datum feature is referenced at MMB or LMB, the system shall compute the datum mobility envelope as the set of rigid transforms of the DRF permitted by the boundary, and the ResultRecord shall reference that envelope.
- **REQ-GDT.DRF-004** Every result evaluated in a DRF shall record the transform used and a boolean stating whether that transform lies within the mobility envelope, with a tolerance on the containment test of 1e-9 m [unverified].
- **REQ-GDT.DRF-005** When a primary planar datum feature rocks (more than one candidate datum set within the simulator's tie tolerance), the system shall enumerate the candidate sets, apply the DMT row's selection rule, and record the candidates and the chosen set.
- **REQ-GDT.DRF-006** If the datum features cannot constrain the degrees of freedom the characteristic requires (for example parallel datum planes as primary and secondary), then the system shall reject the DRF with a typed error naming the degenerate pair.
- **REQ-GDT.DRF-007** DRF construction shall match every ORS-DRF case within the case's stated tolerance, including cases where minimax and constrained L2 simulators disagree.

## Interfaces

- **Offers:** `gdt.build_drf@1` (R1 / W2), `gdt.get_drf@1` (R0), `gdt.define_datum@1` (W2); DRF type with DOF table and mobility envelope; used by `GDT.ZONE`, `GDT.FORM` (orientation-free cases ignore it), `GDT.LOC`, `GDT.PROF`.
- **Consumes:** tangent and constrained fits (KRN); part frame (ALN); DMT rows (GDT.RES); datum and characteristic data (GDT.CHAR).

## Decomposition hints

SIM (datum-feature simulators and candidate sets), BUILD (precedence and DOF bookkeeping), COMMON (common datums and datum targets), MOB (mobility envelope), CHECK (containment and result fields).

## Open questions

- The planar-datum default simulator differs between ISO 5459 editions and ASME Y14.5-2018 [unverified]; each must come from a purchased text and a DMT row, not from this node.

## Children

_None yet: status `open`._
