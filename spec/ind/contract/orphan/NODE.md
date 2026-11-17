---
id: IND.CONTRACT.ORPHAN
title: Missing and disabled pack behaviour
level: 3
parent: IND.CONTRACT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.CONTRACT-010, REQ-IND.CONTRACT-011]
covers: []
depends_on: [IND.CONTRACT.PROV, CORE]
cli: [fid pack list-missing-packs]
mcp: [pack_list_missing_packs]
journal: [pack.list_missing_packs@1]
updated: 2026-11-16
---

# IND.CONTRACT.ORPHAN: Missing and disabled pack behaviour

## Purpose

Keeps projects usable when a pack they used is not installed or is disabled: the project opens, the pack's stored results stay visible read-only with provenance, and any attempt to re-evaluate or edit them is refused with a clear message naming what is missing.

## Scope

**In:**
- Opening a project that references missing or disabled packs
- Read-only presentation data for orphaned elements and results (pack ID, version, algorithm ID, journal entry hash)
- Refusal of re-evaluation and edits of orphaned characteristics
- Preserving orphaned element data unchanged when the project is saved
- `pack.list_missing_packs@1` (R0): the packs a project needs but cannot use

**Out** (and which node owns it instead):
- Project container and load path (owned by `CORE`)
- Replay classification of missing packs (owned by `IND.CONTRACT.PROV`)
- Rendering of read-only elements in the GUI (owned by `VIZ`)

## Requirements

- **REQ-IND.CONTRACT.ORPHAN-001** If a project references a pack that is not installed or is disabled, then the system shall open the project without error and list the missing pack IDs and versions.
- **REQ-IND.CONTRACT.ORPHAN-002** While a pack is missing or disabled, the system shall present that pack's stored results read-only with pack ID, pack version, algorithm ID and journal entry hash.
- **REQ-IND.CONTRACT.ORPHAN-003** While a pack is missing or disabled, the system shall refuse any command that re-evaluates or edits that pack's elements or characteristics with error `E_PACK_UNAVAILABLE` naming the pack ID and version, and shall journal nothing.
- **REQ-IND.CONTRACT.ORPHAN-004** When a project with orphaned pack elements is saved, the system shall write those elements' stored data byte-for-byte unchanged.
- **REQ-IND.CONTRACT.ORPHAN-005** When `fid pack list-missing-packs` runs on a project, the system shall report each referenced pack with its recorded version and its state (installed, disabled, missing or version-mismatched).

## Interfaces

- **Offers:** `pack.list_missing_packs@1` (R0, exposed on MCP); an `OrphanedElement` read-only view type; error `E_PACK_UNAVAILABLE`.
- **Consumes:** CORE project model and command guard; `IND.CONTRACT.PROV` stamps.

## Decomposition hints

OPEN (load path and listing), GUARD (refusals), KEEP (byte-preserving save).

## Open questions

- None beyond CORE.PRJ's handling of unknown element types.

## Children

_None yet: status `open`._
