---
id: KRN.NUM.PRED
title: Exact geometric predicates
level: 3
parent: KRN.NUM
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM-013]
covers: []
depends_on: [KRN.NUM.ERR, KRN.NUM.FN]
uses_artifacts: []
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.PRED: Exact geometric predicates

## Purpose

One shared, deterministic implementation of exact-sign geometric predicates for the kernel and every L0 geometry user: 2D/3D orientation, and in-circle/in-sphere. Each uses a fast f64 filter with an exact (expansion-arithmetic) fallback, no fused multiply-add, and KNP-1 throughout. Added by D-035 because SCAN.MESHOP.CUT, SCAN.MESHOP.REPAIR.DETECT, SCAN.POLY.SURF.FAN and KRN.MZ.VOR all needed it and none owned it.

## Scope

**In:**
- orient2d, orient3d, incircle, insphere on f64 coordinates
- Filter + exact fallback with a documented error-bound derivation
- Determinism and cross-target bit-identity tests
- Migration of the existing predicate code inside KRN.MZ.VOR into this module

**Out:**
- Mesh algorithms that use the predicates (SCAN, GEO)
- Robust intersection construction beyond sign tests (users decide)

## Requirements

- **REQ-KRN.NUM.PRED-001** Each predicate shall return the exact sign of its determinant for every finite f64 input, and shall return a typed error for non-finite input.
- **REQ-KRN.NUM.PRED-002** The predicates shall compile and run without fused multiply-add on every Tier 1 target and the WASM reference artifact, with bit-identical results.
- **REQ-KRN.NUM.PRED-003** KRN.MZ.VOR, SCAN.MESHOP.CUT, SCAN.MESHOP.REPAIR.DETECT and SCAN.POLY.SURF.FAN shall use this module and no other predicate implementation.

## Interfaces

`fid_num::pred::{orient2d, orient3d, incircle, insphere}` returning `Sign` (`Neg | Zero | Pos`) or `NumError::NonFinite`.

## Decomposition hints

Likely a single leaf (size M), or FILTER + EXACT + TESTS.

## Open questions

- Port an established exact-predicate method in clean-room Rust, or vendor a crate that passes the KNP-1 audit? Check licences against ECO.RULES.SPDX.

## Children

_None yet: status `open`._
