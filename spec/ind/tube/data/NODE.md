---
id: IND.TUBE.DATA
title: Tube method declarations and reference datasets
level: 3
parent: IND.TUBE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-007, REQ-IND.TUBE-008]
covers: []
depends_on: [IND.CONTRACT.MANIFEST, IND.CONTRACT.VERIFY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.TUBE.DATA: Tube method declarations and reference datasets

## Purpose

The tube pack's manifest declarations (method IDs, maintainer) and its reference datasets: synthetic multi-bend tubes with known injected deviations, sampled as noisy cylinder points so the whole chain from fit to comparison is exercised.

## Scope

**In:**
- Manifest entries: method IDs `pack.tube.<step>.<method>@<semver>` and maintainer
- Seeded generator: nominal tube, injected LRA deviations, surface points on each straight with seeded noise
- Expected-results files in `fid.pack.expected@1`

**Out** (and which node owns it instead):
- The harness (owned by `IND.CONTRACT.VERIFY`)
- Algorithms under test (owned by the other `IND.TUBE` children)

## Requirements

- **REQ-IND.TUBE.DATA-001** The tube pack manifest shall declare its methods as versioned method IDs and shall name at least one maintainer.
- **REQ-IND.TUBE.DATA-002** The pack shall ship at least one synthetic multi-bend tube with known injected deviations, and noise-free results shall match the injected values within 1e-8 m and 1e-8 rad [unverified].
- **REQ-IND.TUBE.DATA-003** Each synthetic dataset shall record its generator version and seed.

## Interfaces

- **Offers:** manifest fragment and dataset folder for the tube pack.
- **Consumes:** `IND.CONTRACT.MANIFEST` schema; `IND.CONTRACT.VERIFY` format.

## Decomposition hints

GEN (generator), DECL (manifest declarations).

## Open questions

- None.

## Children

_None yet: status `open`._
