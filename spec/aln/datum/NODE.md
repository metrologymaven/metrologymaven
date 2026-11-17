---
id: ALN.DATUM
title: Element-based and datum-system alignment
level: 2
parent: ALN
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN-005]
covers: [GOM.ALN.06, GOM.ALN.07, PCD.ALN.05, PW.ALN.03, PW.ALN.06]
depends_on: [ALN.MODEL, ALN.QA, GDT.RES, GDT.DRF, GEO, KRN]
cli: [fid align run-elements*, fid align run-datum*]
mcp: [align_run_elements*, align_run_datum*]
journal: [align.run_elements*, align.run_datum*]
updated: 2026-11-16
---

# ALN.DATUM: Element-based and datum-system alignment

## Purpose

Aligns data by fitting measured geometric elements to their nominals (any mix of planes, cylinders, circles, points, lines and so on), and aligns to a datum reference frame built from datum features under an explicitly chosen dialect: ASME Y14.5-2018 or ISO 5459:2011. It is separate from `ALN.RPS` because the elements contribute through their intrinsic constraints and datum precedence, not through fixed axis-by-axis locks.

## Scope

**In:**
- Alignment by geometric elements: measured-to-nominal pairs of fitted features, each constraining the DoF its type implies
- Datum-system alignment: primary, secondary and tertiary datum features with precedence, under ASME or ISO
- Datum feature simulators and the order of constraint application per dialect, as supplied by GDT's interpretation resolver
- Recording of the dialect, standard edition and defaults profile on the alignment
- Reporting of residual (unconstrained) DoF when the datum features do not lock all six

**Out** (and which node owns it instead):
- Datum mobility, material modifiers on datums and their effect on tolerance results (`GDT`)
- The interpretation resolver and defaults profiles (`GDT`)
- Fitting the individual features (`KRN`)
- Fixed-direction RPS and 3-2-1 (`ALN.RPS`)
- Residual statistics (`ALN.QA`)

## Requirements

- **REQ-ALN.DATUM-001** When the user runs an element-based alignment, the system shall accept any combination of supported measured/nominal feature pairs and shall report, for each pair, the degrees of freedom it constrains.
- **REQ-ALN.DATUM-002** If the supplied elements constrain fewer than six degrees of freedom, then the system shall report the unconstrained DoF and either keep them from the parent alignment or reject the command, as the user selects.
- **REQ-ALN.DATUM-003** A datum-system alignment shall refuse to run unless the dialect (ASME or ISO), the standard edition and the defaults profile are stated explicitly.
- **REQ-ALN.DATUM-004** The system shall apply datum features in their stated precedence, so that a lower-precedence datum never changes a DoF already constrained by a higher-precedence datum.
- **REQ-ALN.DATUM-005** The datum alignment shall obtain the datum-feature simulator and constraint order from GDT's interpretation resolver and record that operator chain's digest on the alignment.
- **REQ-ALN.DATUM-006** Given the published datum-alignment fixtures with a known transform, the recovered frame shall match within 1e-9 m and 1e-9 rad for noise-free data [unverified].
- **REQ-ALN.DATUM-007** Datum alignment shall be computed by the CPU kernel under KNP-1 and declare its determinism class in the registry.

## Interfaces

- **Offers:** `align.run_elements@1` and `align.run_datum_system@1` (both W2), producing an `Alignment` element; the datum-frame alignment as the starting frame that GDT then evaluates with mobility.
- **Consumes:** GDT interpretation resolver and datum-feature definitions; KRN fitted features; GEO frames; `ALN.MODEL`; `ALN.QA`.

## Decomposition hints

ELEM (feature-pair alignment and DoF accounting), ASME (datum system per ASME Y14.5-2018), ISO (datum system per ISO 5459:2011), RESID (unconstrained-DoF handling).

## Open questions

- GDT depends on ALN and this node depends on GDT's resolver, which risks a cycle. Proposed resolution: GDT exposes the resolver as a lower-level module that ALN.DATUM depends on, while GDT's evaluation modules depend on ALN. Raise in GDT's expansion; no edit made there.
- ISO GPS is a P1b dialect in the constitution; the ISO half of this node may need to be scheduled P1b by decision.

## Children

_None yet: status `open`._
