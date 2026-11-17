---
id: UNC.INST.LIB
title: Instrument-profile library and instrument and gauge register
level: 3
parent: UNC.INST
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-008, REQ-UNC.INST-002]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, UNC.INST.MPE]
cli: [fid unc list-profile, fid unc show-profile, fid unc *-instrument]
mcp: [unc_list_profile, unc_show_profile, unc_*_instrument]
journal: [unc.list_profile@1, unc.*_instrument@1]
updated: 2026-11-17
---

# UNC.INST.LIB: Instrument-profile library and instrument and gauge register

## Purpose

A curated, versioned library of specification-derived instrument profiles for common CMMs, arms, trackers and scanners, each built from publicly published MPE values and citing its public source, offered so a first result can be UEC-1 instead of UEC-0. Agreed at review, this node also owns the **single instrument and gauge register**: the site's own instruments and hand gauges with identity, resolution, measuring range, MPE (through a profile), calibration due date and certificate reference. HAL (file and driver acquisition) and RPT (manual readings) reference register entries; the first-run wizard (FLOW.SETUP step 5) binds to the register's write commands.

## Scope

**In:**
- Library release format: a set of `instrument-profile/v1` descriptors with their `instrument-model/v1` models and a release digest
- Curation rules: public source only (maker datasheet URL and retrieval date), no copied text, values transcribed with units
- `unc.list_profile@1` and `unc.show_profile@1` (R0)
- First release of at least 10 profiles
- The instrument and gauge register: entries, write commands, read commands and query functions for HAL and RPT

**Out** (and which node owns it instead):
- Mapping tables (owned by `UNC.INST.MPE`)
- Maker-signed IEMPS (owned by `UNC.INST.IEMPS`)
- The `instrument-profile/v1` descriptor format (owned by `HAL.IDL.PROFILE`)
- Binding a device instance to a model (owned by `UNC.INST.SCHEMA`)
- Live gauge input and binding readings at runtime (owned by `HAL.DRV`); manual reading entry (owned by `RPT.BALLOON.ENTRY`)

## Requirements

- **REQ-UNC.INST.LIB-001** The first library release shall contain at least 10 profiles covering at least one CMM, arm, tracker and scanner each, every one citing a public source with URL and retrieval date [unverified: availability of public MPE data].
- **REQ-UNC.INST.LIB-002** Every profile shall be derived through `UNC.INST.MPE` mappings and shall be kind `spec_derived` (UEC-1).
- **REQ-UNC.INST.LIB-003** When `unc.list_profile@1` is called, the system shall return profile IDs, device class, maker, model name, source and digest, filterable by class and maker.
- **REQ-UNC.INST.LIB-004** If a profile's source becomes unavailable or is corrected, then the library shall publish a new version and keep the old profile resolvable by digest.
- **REQ-UNC.INST.LIB-005** The system shall keep one instrument and gauge register whose entries hold identity, kind, resolution with unit, measuring range, optional profile reference, calibration due date, certificate reference and status, and every change to it shall be a journaled W2 command.
- **REQ-UNC.INST.LIB-006** The register shall answer, for an entry and a date, whether calibration was overdue on that date, and the profile digest to cite for that entry (or `none`).

## Interfaces

- **Offers:** library releases `instrument-profile-library/v1`; `unc.list_profile@1`, `unc.show_profile@1`; register commands `unc.add_instrument@1`, `unc.update_instrument@1`, `unc.retire_instrument@1` (W2), `unc.list_instrument@1`, `unc.show_instrument@1` (R0); query functions `calibration_overdue`, `resolution_decimals`, `profile_ref` for `HAL.ACQ.FILEDEV`, `HAL.DRV.GAU` and `RPT.BALLOON.ENTRY`.
- **Consumes:** model format (UNC.INST.SCHEMA); mappings (UNC.INST.MPE); descriptor format (HAL.IDL.PROFILE).

## Decomposition hints

FORMAT (release format and profile reads), REG (register), CURATE (first profile set).

## Open questions

- Whether maker trademarks and model names may be used in profile IDs needs counsel review (nominative use) [unverified]. Carried into `UNC.INST.LIB.CURATE`.
- REQ-005 and REQ-006 were added at expansion to record the review agreement that this node owns the register. `UNC.INST`'s Scope and CLI patterns (`*-instrument-model`, `*-profile`) do not yet mention the register or `*-instrument` commands; the parent's owner should widen them.
- `RPT.BALLOON.GAUGE` still specifies its own `report.*_gauge` commands; under the review agreement it shrinks to a view over this register (cross-domain note for RPT).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.INST.LIB.FORMAT | Profile library release format and profile reads (leaf) | 002 003 004 | SA.UNC.01 |
| UNC.INST.LIB.REG | Instrument and gauge register (leaf) | 005 006 | SA.UNC.01 |
| UNC.INST.LIB.CURATE | First curated profile set | 001 002 | SA.UNC.01 |
