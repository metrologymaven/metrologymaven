---
id: IND.PATTERN.FIND
title: Feature-pattern recognition
level: 3
parent: IND.PATTERN
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PATTERN-002, REQ-IND.PATTERN-003]
covers: [GOM.SPE.06]
depends_on: [IND.CONTRACT.MANIFEST, GEO, GDT]
cli: [fid pack find-patterns]
mcp: [pack_find_patterns]
journal: [pack.find_patterns@1]
updated: 2026-11-16
---

# IND.PATTERN.FIND: Feature-pattern recognition

## Purpose

Finds patterns of like features on nominal geometry or PMI: groups features of the same type whose size and orientation agree within declared tolerances, classifies each group as linear, circular, grid or free, and orders members deterministically so later instantiation and summaries are stable across runs and platforms.

## Scope

**In:**
- Candidate features from nominal CAD (holes, studs, slots, nuts) and from PMI pattern callouts
- Grouping by type, size and orientation within declared tolerances
- Pattern kind classification (linear, circular, grid, free) with fitted pattern parameters (pitch, axis, rows and columns)
- Deterministic member ordering
- `pack.find_patterns@1` (R1)

**Out** (and which node owns it instead):
- Nominal feature extraction from B-rep and PMI (owned by `GEO` and `GDT`)
- Creating elements and characteristics (owned by `IND.PATTERN.INST`)
- Feature extraction from scans (owned by `SCAN`)

## Requirements

- **REQ-IND.PATTERN.FIND-001** When pattern recognition runs on nominal geometry, the pack shall group features whose type matches and whose size and axis orientation agree within the declared tolerances.
- **REQ-IND.PATTERN.FIND-002** The pack shall classify each group as linear, circular, grid or free, and shall report the fitted pattern parameters for the first three kinds.
- **REQ-IND.PATTERN.FIND-003** The same nominal geometry and parameters shall produce the same groups, the same kinds and the same member order on every run and platform, independent of input feature order.
- **REQ-IND.PATTERN.FIND-004** The pack shall leave decoy features (same type but outside the size or orientation tolerance) out of every group and shall report them as ungrouped.
- **REQ-IND.PATTERN.FIND-005** When a PMI pattern callout exists for a set of features, the pack shall report the callout's group and flag any disagreement with the geometric grouping.

## Interfaces

- **Offers:** `pack.find_patterns@1` (R1) returning `PatternGroup { id, kind, params, members }`, used by `IND.PATTERN.INST`.
- **Consumes:** GEO nominal features; GDT characteristic model and PMI.

## Decomposition hints

GROUP (type, size, orientation clustering with deterministic order), CLASSIFY (pattern kind and parameters), PMI (callout cross-check).

## Open questions

- Pattern ID derivation (UUIDv5 from sorted member feature IDs is a candidate) should be fixed here because INST derives member IDs from it.

## Children

_None yet: status `open`._
