---
id: IND.SHEET.DATA
title: Sheet metal method declarations and reference datasets
level: 3
parent: IND.SHEET
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.SHEET-008, REQ-IND.SHEET-009]
covers: []
depends_on: [IND.CONTRACT.MANIFEST, IND.CONTRACT.VERIFY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.SHEET.DATA: Sheet metal method declarations and reference datasets

## Purpose

The sheet metal pack's manifest declarations (method IDs, any OEM or national standard by designator and edition only, maintainer) and its synthetic reference datasets for holes, slots, trim points, springback and hems.

## Scope

**In:**
- Manifest entries: method IDs `pack.sheet.<element>.<method>@<semver>`, cited standards by designator and edition only, maintainer
- Synthetic datasets with seeded generators: holes and slots on a plate, trim points with known offsets, a formed panel with known springback per region, a hem section
- Expected-results files in `fid.pack.expected@1`

**Out** (and which node owns it instead):
- The harness (owned by `IND.CONTRACT.VERIFY`)
- Algorithms under test (owned by the other `IND.SHEET` children)

## Requirements

- **REQ-IND.SHEET.DATA-001** The sheet metal pack manifest shall declare every element definition as a versioned method ID, cite any OEM or national standard by designator and edition only, and name at least one maintainer.
- **REQ-IND.SHEET.DATA-002** The pack shall ship synthetic reference datasets covering holes, slots, trim points, a springback panel and a hem section, each with expected values known within 1e-6 m [unverified].
- **REQ-IND.SHEET.DATA-003** Each synthetic dataset shall record its generator version and seed.

## Interfaces

- **Offers:** manifest fragment and dataset folder for the sheet pack.
- **Consumes:** `IND.CONTRACT.MANIFEST` schema; `IND.CONTRACT.VERIFY` format.

## Decomposition hints

One generator per element family.

## Open questions

- A real stamped-part scan with a licence-clear source would strengthen validation; none identified yet.

## Children

_None yet: status `open`._
