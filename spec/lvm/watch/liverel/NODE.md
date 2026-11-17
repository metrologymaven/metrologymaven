---
id: LVM.WATCH.LIVEREL
title: Live surface and feature relationships
level: 3
parent: LVM.WATCH
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.WATCH-006]
covers: [SA.BLD.04]
depends_on: [KRN.CONS, LVM.WATCH.PIPE]
cli: [fid build create-live-relationship, fid build remove-live-relationship]
mcp: [build_create_live_relationship, build_remove_live_relationship]
journal: [build.create_live_relationship@1, build.remove_live_relationship@1]
updated: 2026-11-16
---

# LVM.WATCH.LIVEREL: Live surface and feature relationships

## Purpose

Binds a KRN relationship (point to surface, point to feature, point to point) to a live instrument, so that the relationship's value updates with every new sample. It guarantees that live and offline use the same relationship operation, so a captured reading gives the same value offline as it showed live.

## Scope

**In:**
- Live relationship definition: relationship type, live station, nominal or measured target feature
- Per-sample evaluation through the KRN relationship operation
- Equivalence between live evaluation and offline re-evaluation of a captured reading

**Out** (and which node owns it instead):
- Relationship geometry and operations (`KRN.CONS`)
- Streaming (`LVM.WATCH.PIPE`)
- Capturing (`LVM.WATCH.CAPTURE`)

## Requirements

- **REQ-LVM.WATCH.LIVEREL-001** When `build.create_live_relationship@1` runs, the system shall bind one KRN relationship to a live station and a target feature.
- **REQ-LVM.WATCH.LIVEREL-002** For every published sample, the system shall evaluate the relationship by calling the same KRN relationship operation and version that offline evaluation uses.
- **REQ-LVM.WATCH.LIVEREL-003** Given a captured reading, re-evaluating its relationship offline shall give a value bit-identical to the value published live for that sample on the same platform.
- **REQ-LVM.WATCH.LIVEREL-004** If the target feature changes (for example is re-fitted), then the live relationship shall use the new feature from the next sample and the change shall be visible in the readout.

## Interfaces

- **Offers:** live relationship commands (W2); live relationship values on the `LVM.WATCH.PIPE` stream.
- **Consumes:** `KRN.CONS` relationship operations; `LVM.WATCH.PIPE`.

## Decomposition hints

Possibly a single leaf once `KRN.CONS` fixes its relationship API.

## Open questions

- None yet.

## Children

_None yet: status `open`._
