---
id: IND.PATTERN
title: Feature-pattern pack
level: 2
parent: IND
kind: subsystem
status: split
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND-001, REQ-IND-002, REQ-IND-004]
covers: [GOM.SPE.06]
depends_on: [IND.CONTRACT, GDT, SCAN]
cli: [fid pack *-pattern*]
mcp: [pack_*_pattern*]
journal: [pack.*_pattern*]
updated: 2026-11-16
---

# IND.PATTERN: Feature-pattern pack

## Purpose

Bulk work on repeated features: recognise patterns of like features (holes, studs, slots, nuts) on nominal geometry, instantiate their inspection elements and characteristics in one step, and summarise results per pattern. Parts with hundreds of holes are common in sheet metal and aerostructures, and creating them one by one is the workflow incumbents remove.

## Scope

**In:**
- Pattern recognition on nominal CAD or on PMI: grouping features by type, size and orientation within declared tolerances
- Linear, circular, grid and free patterns
- Batch instantiation of elements and characteristics for every pattern member, using an element type supplied by core or by another pack
- Pattern-level result summaries (counts conforming, worst member, per-member table)
- Pattern report template

**Out** (and which node owns it instead):
- Pattern position tolerances and composite tolerances (owned by `GDT`)
- Sheet-metal hole and slot element definitions (owned by `IND.SHEET`)
- Feature extraction from scans (owned by `SCAN`)
- Pack manifest, verification harness and provenance (owned by `IND.CONTRACT`)

## Requirements

- **REQ-IND.PATTERN-001** The feature-pattern pack shall register its commands, element types, checks and report templates only through the `IND.CONTRACT` manifest.
- **REQ-IND.PATTERN-002** When pattern recognition runs on nominal geometry, the pack shall group features whose type matches and whose size and orientation agree within the declared tolerances, and shall report each group with its pattern kind (linear, circular, grid or free).
- **REQ-IND.PATTERN-003** Pattern recognition shall be deterministic: the same nominal geometry and parameters shall produce the same groups in the same member order on every run and platform.
- **REQ-IND.PATTERN-004** When a recognised pattern is instantiated, the pack shall create one element and one characteristic set per member in a single journaled command, with member IDs derived as UUIDv5 from the pattern ID and member index.
- **REQ-IND.PATTERN-005** When an agent requests pattern instantiation, the resulting elements and characteristics shall be written to an agent branch and provenance-tagged, per CONSTITUTION C5.
- **REQ-IND.PATTERN-006** When pattern members are evaluated, the pack shall emit a pattern summary that counts members per decision outcome and identifies the worst member, without changing any member's own ResultRecord.
- **REQ-IND.PATTERN-007** The feature-pattern pack manifest shall declare its recognition and summary methods as versioned method IDs and shall name a maintainer.
- **REQ-IND.PATTERN-008** The feature-pattern pack shall ship reference datasets including a CAD part with at least 200 holes in linear, circular and grid patterns plus decoy features, with the expected grouping.

## Interfaces

- **Offers:** commands such as `pack.find_patterns@1` (R1), `pack.create_pattern_checks@1` (W2), `pack.summarize_pattern@1` (R1); pattern report template.
- **Consumes:** GEO/IO nominal features and PMI via GDT characteristic model; element types from core or other packs (for example `IND.SHEET`); IND.CONTRACT manifest and provenance.

## Decomposition hints

FIND (recognition), INST (batch instantiation), SUM (pattern summaries), DATA, TPL.

## Open questions

- REQ-IND-003 does not name a pattern pack among those needing a named maintainer, so this node does not satisfy it; it still names a maintainer through its manifest (REQ-007).

## Split rationale

The split follows the **three steps a user takes** with a repeated feature: find the patterns (FIND), create checks for every member at once (INST), and read one summary per pattern (SUM), plus the pack's declarations and datasets (DATA) and template (TPL). REQ-IND.PATTERN-003 (determinism) sits with FIND because member order is fixed there; INST and SUM inherit it.

Alternatives considered:
- **Merge FIND and INST.** Rejected: recognition is R1 and reviewable before any document change, while instantiation is W2 and lands on an agent branch when an agent asks; keeping them apart preserves that review step.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IND.PATTERN.FIND | Feature-pattern recognition | 002 003 | GOM.SPE.06 |
| IND.PATTERN.INST | Batch instantiation of pattern checks | 004 005 | GOM.SPE.06 |
| IND.PATTERN.SUM | Pattern result summary (leaf) | 006 | GOM.SPE.06 |
| IND.PATTERN.DATA | Pattern method declarations and reference datasets | 007 008 | none |
| IND.PATTERN.TPL | Pattern report template | 001 | none |
