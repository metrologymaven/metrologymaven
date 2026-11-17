---
id: KRN.CONS.ELEM.FITC
title: Best fit through feature centres and element points
level: 4
parent: KRN.CONS.ELEM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.ELEM-002, REQ-KRN.CONS.ELEM-004, REQ-KRN.CONS.ELEM-005]
covers: [PCD.CON.01, PW.CON.04]
depends_on: [KRN.CONS.DECL, KRN.CONS.ELEM.SET, KRN.LSQ.LIN, KRN.LSQ.RND, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct fit-centres]
mcp: [construct_fit_centres]
journal: [construct.fit_centres@1]
updated: 2026-11-17
---

# KRN.CONS.ELEM.FITC: Best fit through feature centres and element points

## Purpose

Builds a primitive by least-squares fit through points derived from other elements: the centres of holes for a bolt circle, the centres of spheres for a plane, the points of several point elements for a line. It derives an ordered point set from the inputs, calls the matching `KRN.LSQ` fit and carries that fit's result, diagnostics and algorithm ID inside the constructed element.

## Scope

**In:** derivation rules (centre of circle, sphere, slot, rectangle, ellipse; point of a point element; apex of a cone); inputs as an element list or a constructed set; target primitives line (2D in a given plane, 3D), plane, circle (2D in a given plane, 3D) and sphere; `construct.fit_centres.ls@1.0.0`; the command `construct.fit_centres@1`; a spec card.
**Out:** the fits (`KRN.LSQ.LIN`, `KRN.LSQ.RND`); minimum-zone or envelope fits through centres (later versions); pattern position tolerancing (`GDT`); constructed sets (`KRN.CONS.ELEM.SET`).

## Requirements

- **REQ-KRN.CONS.ELEM.FITC-001** The module shall derive one point per input element in input order by the documented derivation rule, and shall return `WrongInputKind{index}` for an element with no derivable point.
- **REQ-KRN.CONS.ELEM.FITC-002** The module shall fit the requested primitive by calling the corresponding `KRN.LSQ` least-squares fit on the derived points and shall carry its fitted feature, residuals, diagnostics and inner algorithm ID in the constructed element.
- **REQ-KRN.CONS.ELEM.FITC-003** If the derived point count is below the fit's minimum, then the command shall return `InputCount{expected, got}`; fit errors shall be returned wrapped with the inner algorithm ID.
- **REQ-KRN.CONS.ELEM.FITC-004** If weights are supplied, then the command shall return `weights_unsupported` until the fit contract offers weighted fits.
- **REQ-KRN.CONS.ELEM.FITC-005** When the command is committed, the system shall journal one `construct.fit_centres@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/fitc-boltcircle-v1` (8 hole circles on a 100 mm pitch circle with seeded 2 micrometre centre noise), when `fid construct fit-centres --primitive circle3d --json` runs, then the result equals `fit.circle3d.ls` on the 8 centres bit for bit and carries its algorithm ID and diagnostics.
- **AC-2** Given the same 8 circles passed as a constructed set, then the output digest equals AC-1's.
- **AC-3** (error path) Given two circles and `--primitive circle3d`, then the command returns `InputCount{expected: 3, got: 2}` and journals nothing; given a plane among the inputs, it returns `WrongInputKind{index}`.
- **AC-4** Given one input circle replaced, when the `KRN.CONS.DECL` harness runs, then recompute equals a fresh construction bit for bit.
- **AC-5** Given a committed AC-1 run, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `construct.fit_centres@1`, W2 (dry-run R1). Params: inputs (element handles or a set handle), `primitive`, optional projection plane (2D fits), inner `FitParams`. Result: `FittedFeature` with the inner `FitResult`.
- **Algorithm:** derive points into a contiguous buffer, build a `PointsView`, call the `fid-fit` algorithm table entry for `fit.<primitive>.ls`.
- **Errors:** `WrongInputKind`, `InputCount`, `weights_unsupported`, `Fit{inner_algorithm, error}`.
- **Determinism:** D0.
- **Performance budget:** dominated by the inner fit; derivation < 1 microsecond per element [unverified until measured].
- **Crate:** `fid-construct` (module `elem_fitc`).

## Test fixtures

`fixtures/cons/fitc-boltcircle-v1`: seeded hole pattern generator, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `elem_fitc` implementing the `Construction` trait for `construct.fit_centres.ls@1.0.0`. Derive one point per input element in order (centre of circle, sphere, slot, rectangle, ellipse; point element; cone apex), else `WrongInputKind{index}`; accept an element list or a constructed set (resolved in member order); call `fid-fit`'s `fit.line2d|line3d|plane|circle2d|circle3d|sphere.ls` on the derived `PointsView` and carry its result, diagnostics and inner algorithm ID; reject weights with `weights_unsupported`. Add the registry data row `construct.fit_centres@1` (W2, dry-run R1, D0; CLI `fid construct fit-centres`; MCP `construct_fit_centres`; one journal event with algorithm ID, input element IDs, parameter digest, output digest) and a spec card. Tests: `fixtures/cons/fitc-boltcircle-v1` equality with the direct fit, set-versus-list digest equality, input-count and wrong-kind errors, recompute harness, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.ELEM.FITC.

## Open questions

- Weighting centres by their uncertainty is a `UNC` question and needs weighted fits in `KRN.LSQ.TRAIT`.

## Children

_Leaf: none._
