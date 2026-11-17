---
id: KRN.CONS.REL
title: Live relationships
level: 3
parent: KRN.CONS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-008, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [SA.REL.02, SA.REL.03]
depends_on: [KRN.CONS.DECL, KRN.NUM.KDEC]
cli: [fid construct relate-elements]
mcp: [construct_relate_elements]
journal: [construct.relate_elements@1]
updated: 2026-11-16
---

# KRN.CONS.REL: Live relationships

## Purpose

Point-to-object and object-to-object relationships (distance, angle, deviation along a direction) as constructions whose output is a value. Because they are constructions, they recompute when either element changes, which is how SpatialAnalyzer-style live relationships behave. They can carry an optional nominal and tolerance for reporting, without making a conformance decision.

## Scope

**In:**
- Point-to-point, point-to-line, point-to-plane, point-to-circle, point-to-cylinder, point-to-sphere distances (signed where defined); line-line, line-plane and plane-plane angles and distances; deviation along a given direction
- Optional nominal value and tolerance limits held as exact decimals (`KRN.NUM.KDEC`) and carried through to `RPT` and `UNC`
- The registry command and spec cards per relationship variant

**Out** (and which node owns it instead):
- Conformance decisions on relationship tolerances (`UNC`) and their presentation (`RPT`)
- Point-to-mesh and point-to-CAD distances (`GEO.DIST`, `SCAN`)
- Legacy dimensions of GD&T and size (`GDT`)

## Requirements

- **REQ-KRN.CONS.REL-001** The module shall compute each relationship in its published variant table as a `Construction` whose output is a value with quantity kind and SI unit.
- **REQ-KRN.CONS.REL-002** When either input element changes, recomputing the relationship shall give the same value bit for bit as computing it fresh.
- **REQ-KRN.CONS.REL-003** A relationship shall carry an optional nominal and tolerance limits as exact decimals with unit, and shall never convert them to f64 or make a conformance decision.
- **REQ-KRN.CONS.REL-004** If a relationship is undefined (for example the angle between a point and a plane), then it shall return `Undefined`.
- **REQ-KRN.CONS.REL-005** When the command is committed, the system shall journal one `construct.relate_elements@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Interfaces

- **Offers:** `construct.relate_elements@1` (W2, dry-run R1); relationship values for `GDT`, `RPT`, `UNC` and `LVM`.
- **Consumes:** `KRN.CONS.DECL`, `KRN.NUM.KDEC`.

## Decomposition hints

DIST (distances), ANG (angles), DEV (deviation along a direction), TOL (nominal and tolerance attributes).

## Open questions

- Phase: P1a per the concept baseline; SpatialAnalyzer users need relationships by P3 at the latest [unverified].

## Children

_None yet: status `open`._
