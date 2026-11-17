---
id: ALN.DATUM.ISO
title: ISO 5459 datum-system alignment enablement
level: 3
parent: ALN.DATUM
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM-003, REQ-ALN.DATUM-006, REQ-ALN.DATUM-007]
covers: [GOM.ALN.07, PW.ALN.06]
depends_on: [ALN.DATUM.SYS, GDT.RES, GDT.DRF]
cli: [fid align run-datum-system]
mcp: [align_run_datum_system]
journal: [align.run_datum_system@1]
updated: 2026-11-16
---

# ALN.DATUM.ISO: ISO 5459 datum-system alignment enablement

## Purpose

Enables the ISO 5459:2011 dialect in `align.run_datum_system@1`: the dialect-specific differences the engine must honour (default association criteria for datum features, common datums and datum targets as ISO expresses them, and the ISO defaults profile), the ISO fixtures with known transforms, and the switch from `unsupported` to supported once GDT's ISO DMT rows exist. It adds no new command; it extends `ALN.DATUM.SYS`.

## Scope

**In:**
- ISO-specific handling in the engine where the resolver's operator chain requires it
- ISO datum-alignment fixtures and their known transforms
- Checks that ISO and ASME runs on the same geometry record different chains where the dialects differ

**Out** (and which node owns it instead):
- ISO DMT rows (`GDT.RES`, authored per D-009)
- The engine, gate and recording (`ALN.DATUM.SYS`)
- ISO GPS evaluation of tolerances (`GDT`)

## Requirements

- **REQ-ALN.DATUM.ISO-001** While GDT's DMT reports ISO 5459 rows as `unsupported`, a datum-system run with dialect ISO shall fail with `UNSUPPORTED` and the row key, and journal nothing.
- **REQ-ALN.DATUM.ISO-002** When ISO rows are supported, a datum-system run with dialect ISO, edition 2011 and an ISO defaults profile shall use the simulators and order named by those rows and record them as in REQ-ALN.DATUM.SYS-004.
- **REQ-ALN.DATUM.ISO-003** Given the published ISO datum-alignment fixtures with a known transform, the recovered frame shall match within 1e-9 m and 1e-9 rad for noise-free data [unverified].
- **REQ-ALN.DATUM.ISO-004** Given a fixture where ASME and ISO default simulators differ, runs under the two dialects shall record different operator-chain digests.
- **REQ-ALN.DATUM.ISO-005** ISO runs shall meet the same KNP-1 and determinism declarations as `ALN.DATUM.SYS`.

## Interfaces

- **Offers:** the ISO path of `align.run_datum_system@1`.
- **Consumes:** `ALN.DATUM.SYS`; GDT ISO DMT rows and `GDT.DRF`.

## Decomposition hints

DIFF (dialect differences the engine must honour, confirmed by a GD&T expert), FIXT (ISO fixtures), GATE (unsupported-to-supported switch).

## Open questions

- Phase P1b follows CONSTITUTION §7 (ISO GPS dialect in P1b) and GDT.RES (ISO rows from P1b). This resolves the parent's phase question without moving ALN.DATUM.
- Which ISO 5459 default associations differ from ASME's for alignment needs a credentialed expert (D-009 process) [unverified].

## Children

_None yet: status `open`._
