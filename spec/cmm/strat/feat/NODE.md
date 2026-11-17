---
id: CMM.STRAT.FEAT
title: Auto-feature strategy types and hit generation
level: 3
parent: CMM.STRAT
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-001, REQ-CMM.STRAT-002]
covers: [PCD.FEA.01, PCD.FEA.02, PCD.FEA.05]
depends_on: [GEO, CMM.STRAT.CHECK, CMM.PROBE.MODEL]
cli: [fid cmm set-strategy, fid cmm preview-strategy]
mcp: [cmm_set_strategy, cmm_preview_strategy]
journal: [cmm.set_strategy@1]
updated: 2026-11-16
---

# CMM.STRAT.FEAT: Auto-feature strategy types and hit generation

## Purpose

Defines one strategy type per discrete-point feature (point, vector point, surface point, edge point, line, plane, circle, cylinder, cone, sphere, slot) and the deterministic generator that turns a nominal feature plus strategy parameters into hit targets (position and approach vector, SI metres) in the feature's nominal frame. This is the core "where to touch" logic engineers author for every feature.

## Scope

**In:**
- Strategy parameter types per feature type: hit count, levels, depth, start and end angle, spacing, direction (inside or outside), sample hits for local surface-normal correction
- Hit-target generation per type, in the nominal feature frame
- Assigning a strategy and a tip to a feature (`cmm.set_strategy@1`, W2) and previewing generated targets (`cmm.preview_strategy@1`, R1)
- Calling the minimum-point and distribution check before a strategy is accepted

**Out** (and which node owns it instead):
- Scan strategies (owned by `CMM.STRAT.SCAN`)
- The minimum-point and distribution check itself (owned by `CMM.STRAT.CHECK`)
- Strategy defaults and libraries (owned by `CMM.STRAT.LIB`)
- Approach, retract and transit moves (owned by `CMM.PATH`)
- Nominal feature extraction from CAD (owned by `CMM.GEN.CAD`)

## Requirements

- **REQ-CMM.STRAT.FEAT-001** The system shall provide a strategy type for each of point, vector point, surface point, edge point, line, plane, circle, cylinder, cone, sphere and slot, each with a typed, bounded parameter set.
- **REQ-CMM.STRAT.FEAT-002** When a strategy is previewed or set, the system shall generate the hit targets (position in metres, unit approach vector) in the feature's nominal frame as a pure function of nominal feature and parameters (class D0).
- **REQ-CMM.STRAT.FEAT-003** For circle, cylinder and cone strategies, the system shall distribute hits over the configured start and end angles at each level, with levels spaced over the configured depth range.
- **REQ-CMM.STRAT.FEAT-004** For surface and vector points with sample hits enabled, the system shall generate the sample-hit targets around the nominal point at the configured radius so the local surface normal can be corrected from them.
- **REQ-CMM.STRAT.FEAT-005** When a strategy is set, the system shall run the minimum-point and distribution check and shall refuse to store a strategy the check rejects.
- **REQ-CMM.STRAT.FEAT-006** If a strategy parameter is outside its declared bound (for example zero hits, depth beyond feature length, start angle equal to end angle), then the system shall reject it with `STRATEGY_PARAM_INVALID` naming the parameter.

## Interfaces

- **Offers:** `cmm.set_strategy@1` (W2), `cmm.preview_strategy@1` (R1); data types MeasurementStrategy, per-type parameter structs; hit-target lists consumed by `CMM.PATH`.
- **Consumes:** `GEO` nominal feature geometry and frames; `CMM.STRAT.CHECK` validation and the HitTarget type; `CMM.PROBE.MODEL` tip references.
- **Journal:** `cmm.set_strategy@1`.

## Decomposition hints

POINT (point, vector, surface and edge points with sample hits), ROUND (circle, cylinder, cone, sphere), FLAT (line, plane, slot), CMD (set and preview commands).

## Open questions

- Edge point and slot strategies may need `GDT` input on which points define the measurand.
- Default sampling counts per feature type are a domain-expert choice; incumbents' defaults are not copied [unverified].

## Children

_None yet: status `open`._
