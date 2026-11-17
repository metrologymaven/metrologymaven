---
id: IND.GAPFL.DATA
title: Gap and flush reference datasets
level: 3
parent: IND.GAPFL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GAPFL-008]
covers: []
depends_on: [IND.CONTRACT.VERIFY, IND.GAPFL.DEF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.GAPFL.DATA: Gap and flush reference datasets

## Purpose

Synthetic two-panel sections, joints and profile readings with known gap and flush for every supported definition, shipped with expected results for `fid pack verify-pack`.

## Scope

**In:**
- Seeded generators for two-panel sections (flat, radiused edges, mismatched heights) and joints with varying gap
- Simulated profile-sensor readings
- Expected-results files in `fid.pack.expected@1`

**Out** (and which node owns it instead):
- The harness (owned by `IND.CONTRACT.VERIFY`)
- Definitions (owned by `IND.GAPFL.DEF`)

## Requirements

- **REQ-IND.GAPFL.DATA-001** The pack shall ship synthetic two-panel sections with known gap and flush for every supported definition, with expected values matched within 1e-7 m [unverified].
- **REQ-IND.GAPFL.DATA-002** The pack shall ship at least one synthetic joint with linearly varying gap and one simulated profile-sensor reading with expected values.
- **REQ-IND.GAPFL.DATA-003** Each synthetic dataset shall record its generator version and seed.

## Interfaces

- **Offers:** dataset folder for the gap and flush pack.
- **Consumes:** `IND.CONTRACT.VERIFY` format.

## Decomposition hints

One generator per data source.

## Open questions

- None.

## Children

_None yet: status `open`._
