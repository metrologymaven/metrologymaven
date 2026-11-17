---
id: IND.PATTERN.INST
title: Batch instantiation of pattern checks
level: 3
parent: IND.PATTERN
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PATTERN-004, REQ-IND.PATTERN-005]
covers: [GOM.SPE.06]
depends_on: [IND.PATTERN.FIND, IND.CONTRACT.PROFILE]
cli: [fid pack create-pattern-checks]
mcp: [pack_create_pattern_checks]
journal: [pack.create_pattern_checks@1]
updated: 2026-11-16
---

# IND.PATTERN.INST: Batch instantiation of pattern checks

## Purpose

Creates, in one journaled command, one inspection element and one characteristic set per member of a recognised pattern, using an element type supplied by core or by another pack (for example `SheetHole`), with member IDs derived deterministically from the pattern.

## Scope

**In:**
- `pack.create_pattern_checks@1` (W2; agents write to an agent branch)
- Element type selection (core or another pack's type) and a characteristic template applied to every member
- UUIDv5 member IDs from pattern ID and member index
- Atomic creation: all members or none

**Out** (and which node owns it instead):
- Recognition (owned by `IND.PATTERN.FIND`)
- The element types themselves (owned by core or by packs such as `IND.SHEET`)
- Pattern tolerances (owned by `GDT`)

## Requirements

- **REQ-IND.PATTERN.INST-001** When a recognised pattern is instantiated, the pack shall create one element and one characteristic set per member in a single journaled command.
- **REQ-IND.PATTERN.INST-002** Member element IDs shall be UUIDv5 values derived from the pattern ID and member index, so re-instantiating the same pattern yields the same IDs.
- **REQ-IND.PATTERN.INST-003** If creating any member fails, then the pack shall create no member and journal nothing, reporting the failing member index and error.
- **REQ-IND.PATTERN.INST-004** When an agent requests instantiation, the elements and characteristics shall be written to an agent branch and provenance-tagged as agent-authored (CONSTITUTION C5).
- **REQ-IND.PATTERN.INST-005** If the requested element type belongs to a pack that is missing or disabled, then the pack shall refuse instantiation naming that pack.

## Interfaces

- **Offers:** `pack.create_pattern_checks@1` (W2).
- **Consumes:** `IND.PATTERN.FIND` groups; element-type registry through the pack profile; CORE document model and agent branches.

## Decomposition hints

Likely one leaf once CORE's batch-command and agent-branch interfaces are fixed.

## Open questions

- Whether one pack may create elements of another pack's type depends on `IND.CONTRACT.PROFILE` allowing cross-pack element use; assumed yes.

## Children

_None yet: status `open`._
