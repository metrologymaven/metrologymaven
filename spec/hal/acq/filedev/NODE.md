---
id: HAL.ACQ.FILEDEV
title: File pseudo-devices and manual-entry provenance
level: 3
parent: HAL.ACQ
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ-001]
covers: []
depends_on: [HAL.ACQ.STAMP, HAL.ACQ.CONV, HAL.IDL.PROFILE, IO.REG]
cli: [fid device enter-reading, fid device show-file-source]
mcp: [device_enter_reading, device_show_file_source]
journal: [device.enter_reading@1, device.acquisition_batch]
updated: 2026-11-16
---

# HAL.ACQ.FILEDEV: File pseudo-devices and manual-entry provenance

## Purpose

Makes observations that did not come from a live driver (imported CMM outputs, arm and tracker exports, scan meshes and clouds, and readings typed in by an operator) enter the platform exactly like live observations: as stamped batches with honest provenance. It is the P1a path that every first-product workflow uses, so it exists before any live driver.

## Scope

**In:**
- A file pseudo-device per import: device identity as declared by the file (untrusted), `source_kind: file`, trust `declared` (or `unknown` when the file asserts nothing)
- Binding an import to a chosen instrument profile by digest
- Wrapping IO reader output into `AcquisitionBatch`es through the stamping path
- Manual entry: `device.enter_reading@1` (W2) with `source_kind: entered`, the entering principal and an optional gauge identity
- `device.show_file_source@1` (R0)

**Out** (and which node owns it instead):
- Parsing file formats (owned by `IO`)
- Gauge drivers and keyboard-wedge capture (owned by `HAL.DRV.GAU`)
- The manual-entry grid UI (owned by `VIZ` and `RPT`)

## Requirements

- **REQ-HAL.ACQ.FILEDEV-001** When an IO reader yields observations, the file pseudo-device shall emit them as stamped batches with `source_kind: file` and trust `declared`, or `unknown` when the file carries no device identity.
- **REQ-HAL.ACQ.FILEDEV-002** When the user chooses an instrument profile at import, the stamp shall reference that profile by digest; when none is chosen, the sensor-model reference shall be empty and the stamp shall say so.
- **REQ-HAL.ACQ.FILEDEV-003** When `device.enter_reading@1` runs, the system shall journal a batch with `source_kind: entered`, the entering principal, value with unit and the stated resolution, and shall never label it a device observation.
- **REQ-HAL.ACQ.FILEDEV-004** Device identity fields read from a file shall be stored as `UntrustedText` and shall never raise trust above `declared`.
- **REQ-HAL.ACQ.FILEDEV-005** Re-importing the same file bytes with the same options and profile shall produce batches with identical payload digests.

## Interfaces

- **Offers:** `device.enter_reading@1` (W2), `device.show_file_source@1` (R0); `FileSource` adapter used by IO import commands.
- **Consumes:** `IO.REG` reader output envelopes; `HAL.ACQ.STAMP`; `HAL.ACQ.CONV`; `HAL.IDL.PROFILE`.

## Decomposition hints

FILESRC (file pseudo-device adapter and stamping), ENTRY (`device.enter_reading@1`; a likely leaf), PROFBIND (profile binding at import).

## Open questions

- Whether `device.enter_reading@1` belongs in HAL or in `RPT` (where the P1a entry grid lives) depends on how `RPT` splits; HAL keeps the provenance rule either way.

## Children

_None yet: status `open`._
