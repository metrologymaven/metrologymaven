---
id: KRN.LSQ.PLAN2D
title: Least-squares slot, rectangle and ellipse fits
level: 3
parent: KRN.LSQ
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ-002, REQ-KRN.LSQ-003, REQ-KRN.LSQ-006, REQ-KRN.LSQ-007, REQ-KRN.LSQ-009, REQ-KRN.LSQ-010]
covers: [GOM.CON.18, PW.CON.03]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.LIN, KRN.LSQ.RND, KRN.SOLV.NLLS]
cli: [fid fit ls-slot, fid fit ls-rectangle, fid fit ls-ellipse]
mcp: [fit_ls_slot, fit_ls_rectangle, fit_ls_ellipse]
journal: [fit.ls_slot@1, fit.ls_rectangle@1, fit.ls_ellipse@1]
updated: 2026-11-16
---

# KRN.LSQ.PLAN2D: Least-squares slot, rectangle and ellipse fits

## Purpose

Planar compound and conic shapes that CMM and scan users measure every day: round-ended slots, rectangles (with optional corner radii) and ellipses. Each is fitted in a caller-given plane by orthogonal distance, which needs segment assignment (which point belongs to which edge) or, for the ellipse, an orthogonal-distance conic fit.

## Scope

**In:**
- Slot: two parallel lines tangent to two equal semicircles; parameters centre, direction, length, width
- Rectangle: four mutually orthogonal lines; optional equal corner radii
- Ellipse: orthogonal-distance fit (not algebraic distance)
- Deterministic point-to-segment assignment and its update during iteration
- Registry commands, spec cards, typed errors for under-sampled edges

**Out** (and which node owns it instead):
- Lines and circles (`KRN.LSQ.LIN`, `KRN.LSQ.RND`)
- Minimum-zone and envelope versions (`KRN.MZ.EXT`, `KRN.ENV`)
- Slots and rectangles constructed from other elements (`KRN.CONS`)

## Requirements

- **REQ-KRN.LSQ.PLAN2D-001** The module shall fit a slot, a rectangle and an ellipse in a caller-given plane by minimising the sum of squared orthogonal in-plane distances.
- **REQ-KRN.LSQ.PLAN2D-002** Point-to-segment assignment shall be a deterministic function of the current estimate (nearest segment, ties by segment index), and the final assignment shall be returned with the result.
- **REQ-KRN.LSQ.PLAN2D-003** The ellipse fit shall minimise geometric (orthogonal) distance, starting from an algebraic conic fit constrained to an ellipse.
- **REQ-KRN.LSQ.PLAN2D-004** If any edge or arc of a slot or rectangle receives fewer points than it needs, or the ellipse's axis ratio is within the configured tolerance of 1, then the fit shall return a typed error naming the segment or the ratio.
- **REQ-KRN.LSQ.PLAN2D-005** Each result shall report diagnostics, residual statistics and per-segment residual statistics.
- **REQ-KRN.LSQ.PLAN2D-006** When a command is committed, the system shall journal one `fit.ls_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.ls_slot@1`, `fit.ls_rectangle@1`, `fit.ls_ellipse@1` (W2, dry-run R1); fitted features used by `KRN.MZ.EXT`, `KRN.ENV`, `GDT`.
- **Consumes:** `KRN.LSQ.TRAIT`, `KRN.LSQ.LIN`, `KRN.LSQ.RND`, `KRN.SOLV.NLLS`.

## Decomposition hints

SLOT, RECT (with corner radii), ELL (orthogonal-distance ellipse, for example the method of Ahn et al. 2001 [unverified reference]).

## Open questions

- No NMI reference data exist for these shapes [unverified]; validation relies on synthetic and ORS cases from `UNC`.
- Phase: P1a here because CMM and scan users need slots early; the concept doc lists slot and ellipse as "later" for least squares.

## Children

_None yet: status `open`._
