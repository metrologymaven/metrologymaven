---
id: IND.GEAR.DATA
title: Gear standards declarations and reference datasets
level: 3
parent: IND.GEAR
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GEAR-002, REQ-IND.GEAR-009]
covers: []
depends_on: [IND.CONTRACT.MANIFEST, IND.CONTRACT.VERIFY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.GEAR.DATA: Gear standards declarations and reference datasets

## Purpose

The gear pack's manifest declarations (standards with editions, method IDs, maintainer) and its reference datasets: synthetic gears with known injected deviations whose expected results are exact by construction.

## Scope

**In:**
- Manifest entries: ISO 1328-1:2013 [unverified edition] and ISO 21771 (edition to be confirmed) [unverified], method IDs `pack.gear.<deviation>.<method>@<semver>`, maintainer
- Synthetic gear generator with seeded injected profile, helix, pitch and runout deviations
- Expected-results files in `fid.pack.expected@1`

**Out** (and which node owns it instead):
- The harness (owned by `IND.CONTRACT.VERIFY`)
- Algorithms under test (owned by `IND.GEAR.DEF`, `IND.GEAR.FLANK`, `IND.GEAR.PITCH`, `IND.GEAR.SIZE`, `IND.GEAR.GRADE`)

## Requirements

- **REQ-IND.GEAR.DATA-001** The gear pack manifest shall declare each gear accuracy standard it implements by designator and edition and shall name at least one maintainer.
- **REQ-IND.GEAR.DATA-002** The pack shall ship at least one synthetic spur gear and one synthetic helical gear with seeded injected deviations, and every computed deviation shall match the injected value within 1e-8 m [unverified].
- **REQ-IND.GEAR.DATA-003** Each synthetic dataset shall record its generator version and seed so it can be regenerated bit-exactly.

## Interfaces

- **Offers:** manifest fragment and dataset folder for the gear pack.
- **Consumes:** `IND.CONTRACT.MANIFEST` schema; `IND.CONTRACT.VERIFY` format.

## Decomposition hints

GEN (synthetic generator), DECL (manifest declarations).

## Open questions

- A measured gear dataset from a licence-clear source (for example a gear artefact calibration) would strengthen validation; none identified yet.

## Children

_None yet: status `open`._
