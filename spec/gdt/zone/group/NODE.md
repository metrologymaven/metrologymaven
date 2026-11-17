---
id: GDT.ZONE.GROUP
title: Patterns and simultaneous requirements
level: 3
parent: GDT.ZONE
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE-003, REQ-GDT.ZONE-007]
covers: []
depends_on: [GDT.ZONE.POSE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.ZONE.GROUP: Patterns and simultaneous requirements

## Purpose

Solves several toleranced features with **one shared pose** of their zone framework: patterns of holes or pins under one position callout (P1a), and simultaneous-requirement groups across several callouts that reference the same datums (P1b on the scope ladder). Each feature's result references the group ID so reports and verifiers can see the shared solve.

## Scope

**In:**
- Group formation from the characteristic model (pattern count, simultaneous-requirement group IDs)
- One pose variable for all zones in a group; per-feature usage
- Group ID in each feature's ResultRecord
- P1a: patterns within one callout; P1b: simultaneous requirements across callouts

**Out** (and which node owns it instead):
- Which callouts form a simultaneous requirement by default (owned by DMT rows in `GDT.RES`)
- Composite tiers (owned by `GDT.ZONE.COMP`)

## Requirements

- **REQ-GDT.ZONE.GROUP-001** When features form a pattern or a simultaneous-requirement group, the engine shall solve all their zones with one shared pose and report per-feature usage at that pose.
- **REQ-GDT.ZONE.GROUP-002** Each feature's ResultRecord shall reference the group ID and the shared pose digest.
- **REQ-GDT.ZONE.GROUP-003** If a group member's evaluation fails (for example missing points), then the whole group shall return `group_incomplete` naming the member, and no member shall get a value.
- **REQ-GDT.ZONE.GROUP-004** The engine shall reproduce every ORS-GDT pattern and simultaneous-requirement case within its stated tolerance.

## Interfaces

- **Offers:** `solve_group(members, drf, constraints) -> GroupSolution`.
- **Consumes:** pose solve (GDT.ZONE.POSE); group membership from the characteristic model (GDT.CHAR.MODEL).

## Decomposition hints

PATTERN (P1a pattern within one callout), SIMREQ (P1b simultaneous requirements), IDS (group IDs and digests; a leaf).

## Open questions

- Whether a failed member should fail the whole group (REQ-003) or report the rest with a flag is a behaviour choice an expert should confirm.

## Children

_None yet: status `open`._
