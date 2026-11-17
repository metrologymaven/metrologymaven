---
id: IND.CONTRACT.PROV
title: Pack provenance stamping and replay
level: 3
parent: IND.CONTRACT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.CONTRACT-007, REQ-IND.CONTRACT-008, REQ-IND.CONTRACT-009]
covers: []
depends_on: [IND.CONTRACT.MANIFEST, CORE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.CONTRACT.PROV: Pack provenance stamping and replay

## Purpose

Makes pack results first-class values of record: the host stamps every ResultRecord a pack command produces with the pack ID and version, and the replay engine compares recorded and installed pack versions so a version change is visible and classified.

## Scope

**In:**
- Host-side stamping of pack ID and pack version on every ResultRecord produced by a pack command
- Carrying the stamp in QIF through a sanctioned extension hook
- Replay with the same pack version (normal CONSTITUTION §4 rules)
- Replay with a different or missing pack version: flagging and outcome classification

**Out** (and which node owns it instead):
- The ResultRecord type, journal and replay engine (owned by `CORE`)
- Showing results of missing packs in projects (owned by `IND.CONTRACT.ORPHAN`)

## Requirements

- **REQ-IND.CONTRACT.PROV-001** When a pack command emits a ResultRecord, the host (not pack code) shall stamp it with the pack ID and pack version taken from the loaded manifest.
- **REQ-IND.CONTRACT.PROV-002** If pack code supplies a ResultRecord whose algorithm ID is not one of the method IDs declared in its manifest, then the host shall reject the result with a typed error and journal nothing.
- **REQ-IND.CONTRACT.PROV-003** The pack stamp shall be written to QIF results through a sanctioned QIF extension hook and shall survive a QIF export and re-import unchanged.
- **REQ-IND.CONTRACT.PROV-004** When a journal containing pack commands is replayed with the recorded pack version installed, the replay outcome shall be determined by CONSTITUTION §4 exactly as for core commands.
- **REQ-IND.CONTRACT.PROV-005** If a journal is replayed with a different version of a pack than recorded, then the system shall flag every result of that pack with the recorded and installed versions and shall classify any changed conformance decision as `DIVERGENT`.
- **REQ-IND.CONTRACT.PROV-006** If a journal is replayed while a pack it uses is missing or disabled, then the system shall classify the affected entries as `NOT_REPLAYABLE` and name the pack and version.

## Interfaces

- **Offers:** a result-emission wrapper for pack commands; a replay hook `pack_version_check`; flags `PACK_VERSION_MISMATCH`, `PACK_MISSING`.
- **Consumes:** CORE ResultRecord, journal and replay engine; IO.QIF extension hooks; `IND.CONTRACT.MANIFEST` method declarations.

## Decomposition hints

STAMP (emission wrapper and QIF hook), REPLAY (version comparison and classification).

## Open questions

- Whether a patch-level pack version change with identical outputs should count as `EQUIVALENT` or `IDENTICAL` needs CORE's replay node to define version-aware outcomes.

## Children

_None yet: status `open`._
