---
id: GDT.DRF.MOB
title: Datum mobility envelope
level: 3
parent: GDT.DRF
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF-003]
covers: [PW.GDT.07]
depends_on: [GDT.DRF.BUILD, KRN.SOLV]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.DRF.MOB: Datum mobility envelope

## Purpose

Computes the set of rigid transforms of a DRF that the datum features permit when they are referenced at MMB or LMB (and, where the row says so, from rocking): the **mobility envelope**. It is represented as a constraint set over the free pose parameters, never as a scalar shift, so `GDT.ZONE` can search within it and `GDT.DRF.CHECK` can test membership.

## Scope

**In:**
- Boundary (MMB / LMB) sizes from the DMT row and the datum feature's actual geometry
- Envelope representation: linear and convex constraints on the DRF pose parameters, with a reference ID and digest
- Envelopes for single datums and for datum patterns moving as a rigid group (`group_id`)

**Out** (and which node owns it instead):
- Searching the envelope during a zone solve (owned by `GDT.ZONE.MOBSRCH`)
- Membership test and result fields (owned by `GDT.DRF.CHECK`)

## Requirements

- **REQ-GDT.DRF.MOB-001** When a datum feature is referenced at MMB or LMB, the system shall compute a mobility envelope as constraints on the DRF's pose parameters derived from the boundary named by the resolved row.
- **REQ-GDT.DRF.MOB-002** Each envelope shall have a stable reference ID and a digest over its canonical form, and the ResultRecord shall reference that ID.
- **REQ-GDT.DRF.MOB-003** When several datum features move as one pattern, the envelope shall constrain them as one rigid group and record the group ID.
- **REQ-GDT.DRF.MOB-004** Envelopes shall match every ORS-DRF mobility case within the case's stated tolerance.

## Interfaces

- **Offers:** `MobilityEnvelope` type and constructor; used by `GDT.ZONE.MOBSRCH` and `GDT.DRF.CHECK`.
- **Consumes:** DRF (GDT.DRF.BUILD); convex solver primitives (KRN.SOLV); DMT rows.

## Decomposition hints

REPR (envelope type and digest; likely a leaf), MMB (boundary-derived constraints for planes and cylinders), GROUP (rigid groups).

## Open questions

- Whether non-convex envelopes (for example rocking plus clearance) must be supported, or approximated conservatively with a flag; needs an expert call through the DMT.

## Children

_None yet: status `open`._
