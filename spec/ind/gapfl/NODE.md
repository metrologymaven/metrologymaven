---
id: IND.GAPFL
title: Gap and flush pack
level: 2
parent: IND
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND-001, REQ-IND-002, REQ-IND-003, REQ-IND-004]
covers: [GOM.SPE.04, PW.IND.02]
depends_on: [IND.CONTRACT, SCAN, HAL]
cli: [fid pack *-gapflush*]
mcp: [pack_*_gapflush*]
journal: [pack.*_gapflush*]
updated: 2026-11-16
---

# IND.GAPFL: Gap and flush pack

## Purpose

Measurement of gap (the opening between two adjacent panels) and flush (their step height) along a joint, as used in automotive closures and aerospace skin panels. Values come from sections through scan data or from profile-sensor (gap-gun style) readings, using an explicitly chosen gap and flush definition.

## Scope

**In:**
- Gap and flush point element: a location on a joint between two named panels, with a section direction
- Selectable gap definitions (for example edge-to-edge, tangent-to-tangent at declared offsets, minimum distance) and flush definitions (step along a declared reference normal)
- Mirrored and car-line conventions for sign (which panel is master)
- Gap and flush along a joint line at declared spacing
- Evaluation from mesh/point sections and from line-profile sensor data
- Parallelism (gap variation) along a joint
- Gap and flush report template

**Out** (and which node owns it instead):
- Section curve extraction from meshes (owned by `SCAN`)
- Profile-sensor drivers and live acquisition (owned by `HAL`)
- Pack manifest, verification harness and provenance (owned by `IND.CONTRACT`)

## Requirements

- **REQ-IND.GAPFL-001** The gap and flush pack shall register its commands, element types, checks and report templates only through the `IND.CONTRACT` manifest.
- **REQ-IND.GAPFL-002** Every gap and flush element shall name its gap definition, flush definition, master panel and sign convention explicitly, and the pack shall refuse an element with any of these unset.
- **REQ-IND.GAPFL-003** When evaluated from a section that contains both panel edges, the gap and flush pack shall report gap and flush in metres according to the element's definitions.
- **REQ-IND.GAPFL-004** If either panel edge cannot be detected in the section within the declared search window, then the pack shall report the element as `NOT_DECIDABLE` and shall state which edge was missing.
- **REQ-IND.GAPFL-005** When evaluated from a profile-sensor reading, the gap and flush pack shall apply the same definitions as for scan sections and shall record the evaluation basis as `DIRECT_READING` or `POINTS` accordingly.
- **REQ-IND.GAPFL-006** When a joint line is evaluated at a declared spacing, the pack shall report gap and flush at each station plus maximum, minimum and range of gap along the joint.
- **REQ-IND.GAPFL-007** The gap and flush pack manifest shall declare its definitions as versioned method IDs (OEM definitions differ and no single international standard is assumed [unverified]) and shall name a maintainer.
- **REQ-IND.GAPFL-008** The gap and flush pack shall ship reference datasets including synthetic two-panel sections with known gap and flush for every supported definition, matched within 1e-7 m [unverified].

## Interfaces

- **Offers:** element types `GapFlushPoint`, `GapFlushJoint`; commands such as `pack.create_gapflush_point@1` (W2), `pack.evaluate_gapflush@1` (R1); gap and flush report template.
- **Consumes:** SCAN sections; HAL Profile device readings; IND.CONTRACT manifest and provenance.

## Decomposition hints

DEF (definitions and sign conventions), SECT (evaluation from sections), SENSOR (evaluation from profile sensors), JOINT (along-joint stations and parallelism), DATA, TPL.

## Open questions

- The catalogue of gap definitions to ship first needs an automotive domain expert; the list in Scope is a starting proposal.

## Children

_None yet: status `open`._
