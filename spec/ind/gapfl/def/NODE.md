---
id: IND.GAPFL.DEF
title: Gap and flush definitions and elements
level: 3
parent: IND.GAPFL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GAPFL-001, REQ-IND.GAPFL-002, REQ-IND.GAPFL-007]
covers: [GOM.SPE.04, PW.IND.02]
depends_on: [IND.CONTRACT.MANIFEST]
cli: [fid pack create-gapflush-point, fid pack create-gapflush-joint]
mcp: [pack_create_gapflush_point, pack_create_gapflush_joint]
journal: [pack.create_gapflush_point@1, pack.create_gapflush_joint@1]
updated: 2026-11-16
---

# IND.GAPFL.DEF: Gap and flush definitions and elements

## Purpose

The catalogue of gap and flush definitions and sign conventions, declared as versioned method IDs, and the element types that must name them explicitly: `GapFlushPoint` (one location on a joint) and `GapFlushJoint` (a joint line with a station spacing and a point template).

## Scope

**In:**
- Gap definitions (proposed: edge-to-edge, tangent-to-tangent at declared offsets, minimum distance) and flush definitions (step along a declared reference normal), each a versioned method ID
- Master-panel choice and sign conventions (including mirrored and car-line conventions)
- Element types `GapFlushPoint` and `GapFlushJoint`, with their create commands (W2; agents write to an agent branch)
- Manifest declarations and maintainer

**Out** (and which node owns it instead):
- Evaluating from scan sections (owned by `IND.GAPFL.SECT`)
- Evaluating from profile sensors (owned by `IND.GAPFL.SENSOR`)
- Evaluating along a joint (owned by `IND.GAPFL.JOINT`)

## Requirements

- **REQ-IND.GAPFL.DEF-001** The gap and flush element types and create commands shall be registered only through the gap and flush pack manifest.
- **REQ-IND.GAPFL.DEF-002** If a gap and flush element is created without a gap definition, flush definition, master panel or sign convention, then the pack shall refuse it with a typed error naming the missing field and journal nothing.
- **REQ-IND.GAPFL.DEF-003** Each gap and flush definition shall be a versioned method ID with a written description in the project's own words, and the manifest shall list every shipped definition.
- **REQ-IND.GAPFL.DEF-004** The sign convention shall state, for gap and for flush, which panel is master and which direction is positive, and mirrored elements shall derive their convention from their source element.
- **REQ-IND.GAPFL.DEF-005** A `GapFlushJoint` shall hold the nominal joint polyline, the station spacing, start and end offsets and a point template naming the definitions, master panel, sign convention, section direction hint and search window.
- **REQ-IND.GAPFL.DEF-006** The gap and flush pack manifest shall name at least one maintainer and shall not claim conformance to any single international standard for its definitions [unverified that none exists].

## Interfaces

- **Offers:** element types `GapFlushPoint`, `GapFlushJoint`; create commands (W2); `GapFlushDefinition` and `SignConvention` types used by SECT, SENSOR and JOINT.
- **Consumes:** `IND.CONTRACT.MANIFEST`; CORE document model.

## Decomposition hints

CATALOG (definitions and sign conventions), ELEM (element types and create commands).

## Open questions

- The catalogue of gap definitions to ship first needs an automotive domain expert; the list in Scope is a starting proposal.

## Children

_None yet: status `open`._
