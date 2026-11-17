---
id: GDT.DRF.CHECK
title: Transform-used and within-envelope result fields
level: 3
parent: GDT.DRF
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF-004]
covers: []
depends_on: [GDT.DRF.BUILD, GDT.DRF.MOB, GDT.CHAR.RECORD]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.CHECK: Transform-used and within-envelope result fields

## Purpose

Every result evaluated in a DRF records the transform actually used and whether that transform lies inside the mobility envelope. This node owns the membership test and the `datum_mobility` result fields, and the `frame: alignment-substitute` flag when a characteristic was evaluated in an alignment rather than its DRF (concept doc 05 §2.4). In P1a (RMB only) the envelope is a single pose and the test is trivial; it becomes substantive with mobility in P1b.

## Scope

**In:**
- Membership test of a transform in a `MobilityEnvelope` with tolerance 1e-9 m and 1e-9 rad [unverified]
- `datum_mobility` fields: DRF reference, transform used, envelope reference, within-envelope boolean, group ID
- `frame: drf | alignment-substitute` flag

**Out** (and which node owns it instead):
- Envelope construction (owned by `GDT.DRF.MOB`)
- ResultRecord builder (owned by `GDT.CHAR.RECORD`)

## Requirements

- **REQ-GDT.DRF.CHECK-001** Every ResultRecord evaluated in a DRF shall carry the transform used and a within-envelope boolean computed with tolerance 1e-9 m and 1e-9 rad [unverified].
- **REQ-GDT.DRF.CHECK-002** While no datum carries MMB or LMB, the envelope shall be the single DRF pose and the boolean shall be true exactly when the transform used equals that pose within tolerance.
- **REQ-GDT.DRF.CHECK-003** If a characteristic is evaluated in an alignment rather than its DRF, then the ResultRecord shall carry `frame: alignment-substitute`.

## Interfaces

- **Offers:** `check_within(transform, envelope) -> bool` and result-field population for every evaluating child.
- **Consumes:** DRF and envelope (siblings); builder (GDT.CHAR.RECORD).

## Decomposition hints

RMB (single-pose case, a leaf for P1a), MEMB (general membership once `GDT.DRF.MOB` fixes the envelope representation), FLAG (alignment-substitute).

## Open questions

- Resolved at split: the RMB-only part and the frame flag are a P1a leaf (`GDT.DRF.CHECK.FIELDS`) that reaches general envelopes only through a `MembershipTest` trait; the general test (`GDT.DRF.CHECK.MEMB`, P1b) uses the representation fixed by `GDT.DRF.MOB.REPR`. The parent's `depends_on: [GDT.DRF.MOB]` therefore applies to MEMB only.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.DRF.CHECK.FIELDS | Datum-mobility result fields, RMB membership and frame flag (leaf) | 001 002 003 | none |
| GDT.DRF.CHECK.MEMB | Membership test of a transform in a mobility envelope (leaf) | 001 | none |
