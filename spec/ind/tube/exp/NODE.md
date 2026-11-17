---
id: IND.TUBE.EXP
title: Bend-data export
level: 3
parent: IND.TUBE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-001]
covers: [PW.IND.04, PCD.IND.04]
depends_on: [IND.TUBE.BEND, IND.TUBE.CORR, IND.CONTRACT.PROFILE]
cli: [fid pack export-tube-benddata]
mcp: [pack_export_tube_benddata]
journal: [pack.export_tube_benddata@1]
updated: 2026-11-16
---

# IND.TUBE.EXP: Bend-data export

## Purpose

Writes nominal, measured or corrected bend data to files for bending machines and tube-shop software, through exporters registered in the pack manifest, mapping the pack's internal LRA convention to each target convention.

## Scope

**In:**
- Exporters registered through the manifest, one per file format, implemented from public specifications only
- Mapping from `fid.lra@1` to target conventions (rotation sign, rotation order, absolute or relative rotation, units)
- `pack.export_tube_benddata@1` (W1, `fs_effect: write_root`, into an approved local export root)

**Out** (and which node owns it instead):
- Bend data itself (owned by `IND.TUBE.BEND` and `IND.TUBE.CORR`)
- Export roots and file-write policy (owned by `IO`)
- Live bender communication (owned by `HAL`; not yet catalogued)

## Requirements

- **REQ-IND.TUBE.EXP-001** Each bend-data exporter shall be registered only through the tube pack manifest.
- **REQ-IND.TUBE.EXP-002** `pack.export_tube_benddata@1` shall write only into an approved local export root and shall be class W1 with `fs_effect: write_root` (CONSTITUTION §3.1).
- **REQ-IND.TUBE.EXP-003** Each exporter shall declare its target convention, and an export followed by the exporter's own reader (where one exists) shall reproduce the LRA data within the format's stated precision.
- **REQ-IND.TUBE.EXP-004** Exporters shall be implemented from public format specifications only (CONSTITUTION §8 clean room), and the manifest shall cite each specification's source.
- **REQ-IND.TUBE.EXP-005** When corrections are exported, the file shall carry an advisory marker where the format allows one.

## Interfaces

- **Offers:** `pack.export_tube_benddata@1` (W1); exporter registrations.
- **Consumes:** `IND.TUBE.BEND` LRA data; `IND.TUBE.CORR` corrections; IO export roots.

## Decomposition hints

One child per file format once formats are chosen, plus CONV (convention mapping).

## Open questions

- Which bender file formats to export first needs a tube-shop expert; formats must be implemented from public specifications only.

## Children

_None yet: status `open`._
