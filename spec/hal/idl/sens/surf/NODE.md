---
id: HAL.IDL.SENS.SURF
title: HeightMap@1 and Profile@1 packages (areal and profile surface data)
level: 4
parent: HAL.IDL.SENS
kind: feature
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.SENS-001]
covers: []
depends_on: [HAL.IDL.SENS.STREAM, HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.SENS.SURF: HeightMap@1 and Profile@1 packages

## Purpose

Two packages for form and texture instruments: `HeightMap@1` for areal topography (optical profilers, focus variation, confocal) and `Profile@1` for stylus profiles and roundness traces. They feed the P3 form and texture work.

## Scope

**In:**
- `HeightMap@1`: grid spacing (m), dimensions, encoded z array, validity mask, metadata compatible with the ISO 25178-72 (X3P) record [unverified]
- `Profile@1`: linear profiles (x spacing, z array) and polar traces (angle step, radial deviation), stylus tip radius, filter state as acquired
- Conformance profiles `HeightMap-core@1` and `Profile-core@1`

**Out:**
- Filtering and parameter evaluation (owned by `KRN` and `GDT`)
- X3P file import and export (owned by `IO`)

## Requirements

- **REQ-HAL.IDL.SENS.SURF-001** A `HeightMap@1` batch shall carry x and y spacing in m, grid dimensions, an encoded z array with target unit m, a validity mask and acquisition metadata fields that map one to one to the X3P record fields named in a mapping table [unverified].
- **REQ-HAL.IDL.SENS.SURF-002** A `Profile@1` trace shall state whether it is linear or polar, its sampling step (m or rad), the stylus tip radius (m) and whether any filter was applied by the instrument.
- **REQ-HAL.IDL.SENS.SURF-003** If the instrument applied a filter, then the trace shall name the filter type and cutoff as reported, as `UntrustedText` plus a parsed value where possible, so later evaluation can refuse double filtering.

## Interfaces

- **Offers:** `HeightMap@1`, `Profile@1` and their profiles.
- **Consumes:** `HAL.IDL.SENS.STREAM`, `HAL.IDL.BASE`, `HAL.IDL.UNITS`, `HAL.IDL.GEN.FMT`.

## Decomposition hints

HMAP (`HeightMap@1` with the X3P field mapping) and PROF (`Profile@1`), each likely an M leaf.

## Open questions

- X3P metadata compatibility is [unverified] against the published ISO 25178-72 schema; a domain expert should confirm the field mapping before a leaf is written.

## Children

_None yet: status `open`._
