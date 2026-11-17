---
id: KRN.CONS.ELEM
title: Primitives built from elements and constructed sets
level: 3
parent: KRN.CONS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-005, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [GOM.CON.01, GOM.CON.02, GOM.CON.03, GOM.CON.04, GOM.CON.05, GOM.CON.06, GOM.CON.07, GOM.CON.08, GOM.CON.09, PCD.CON.01, PCD.CON.03, PW.CON.04]
depends_on: [KRN.CONS.DECL, KRN.LSQ.LIN, KRN.LSQ.RND, KRN.LSQ.AXIS, KRN.LSQ.PLAN2D]
cli: [fid construct build-*, fid construct fit-centres, fid construct make-set]
mcp: [construct_build_*, construct_fit_centres, construct_make_set]
journal: [construct.build_*, construct.fit_centres@1, construct.make_set@1]
updated: 2026-11-16
---

# KRN.CONS.ELEM: Primitives built from elements and constructed sets

## Purpose

Builds a point, line, plane, circle, slot, rectangle, cylinder, cone or sphere from other elements rather than from raw points: by definition (a line through two points, a plane through three, a cylinder from an axis and a radius), by best fit through the centres or points of several features (a bolt-circle through hole centres), or from an ordered constructed set used as one input.

## Scope

**In:**
- Definitional builds per primitive (minimum element counts and roles)
- Best fit through feature centres or element points, delegating to the `KRN.LSQ` fits on the derived point set
- Constructed sets: ordered, typed groups of elements that act as one input, with their own dependency declaration
- Registry commands and spec cards

**Out** (and which node owns it instead):
- Pairwise operations (`KRN.CONS.OPS`)
- Fitting from measured points (`KRN.LSQ`, `KRN.MZ`, `KRN.ENV`)
- Pattern (bolt-circle) position tolerancing (`GDT`)

## Requirements

- **REQ-KRN.CONS.ELEM-001** The module shall build each of point, line, plane, circle, slot, rectangle, cylinder, cone and sphere from a documented minimum set of input elements, each implementing the `Construction` trait.
- **REQ-KRN.CONS.ELEM-002** When a primitive is built by best fit through feature centres or points, the module shall call the corresponding `KRN.LSQ` fit and carry its result, diagnostics and algorithm ID in the constructed element.
- **REQ-KRN.CONS.ELEM-003** A constructed set shall preserve its member order, and changing that order shall change its dependency declaration.
- **REQ-KRN.CONS.ELEM-004** If the inputs are fewer than the documented minimum or of the wrong kind, then the build shall return `InputCount` or `WrongInputKind`.
- **REQ-KRN.CONS.ELEM-005** When a command is committed, the system shall journal one `construct.*` event with algorithm ID, input element IDs, parameter digest and output digest.

## Interfaces

- **Offers:** `construct.build_<primitive>@1`, `construct.fit_centres@1`, `construct.make_set@1` (W2, dry-run R1).
- **Consumes:** `KRN.CONS.DECL`, `KRN.LSQ`.

## Decomposition hints

DEF (definitional builds per primitive), FITC (best fit through centres and points), SET (constructed sets).

## Open questions

- Whether best fit through centres should weight centres by their own uncertainty is a `UNC` question; this node fits unweighted unless weights are given explicitly.

## Children

_None yet: status `open`._
