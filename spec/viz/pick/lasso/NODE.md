---
id: VIZ.PICK.LASSO
title: Lasso selection as semantic queries
level: 3
parent: VIZ.PICK
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK-001, REQ-VIZ.PICK-002, REQ-VIZ.PICK-004, REQ-VIZ.PICK-005, REQ-VIZ.PICK-009]
covers: []
depends_on: [VIZ.PICK.RESOLVE, GEO.SEL, GEO.INDEX]
cli: [fid view resolve-lasso]
mcp: [view_resolve_lasso]
journal: [view.resolve_lasso@*]
updated: 2026-11-16
---

# VIZ.PICK.LASSO: Lasso selection as semantic queries

## Purpose

Rectangle, polygon and brush lassos over clouds, meshes and CAD, with through-selection and visible-only modes. A lasso is recorded as a geometric query (for example points of a cloud inside a prism defined by a polygon, an f64 camera and a depth range), which the CPU resolves deterministically into an index set; the query, not the click, is what replays on new data.

## Scope

**In:**
- Rectangle, polygon and brush gestures turned into prism or swept-volume queries in f64
- Through (all depths) and visible-only (front-most by f64 depth test) modes
- `view.resolve_lasso@1` (R1) returning an index set and the semantic query
- Resolution over 10⁸ points within budget, with deterministic order
- Re-resolution of the query on new data for re-targeted replay

**Out** (and which node owns it instead):
- Selection-set algebra and storage (`VIZ.PICK.SELSET`)
- Query types and spatial indexes (`GEO.SEL`, `GEO.INDEX`)
- Single picks (`VIZ.PICK.RESOLVE`)

## Requirements

- **REQ-VIZ.PICK.LASSO-001** When a lasso completes, the system shall convert it into a semantic query in f64 world terms (polygon in normalised view coordinates, f64 camera, depth range, target entity IDs) before any resolution.
- **REQ-VIZ.PICK.LASSO-002** The CPU shall resolve the query into a sorted index set per target entity, and the GPU shall not decide membership.
- **REQ-VIZ.PICK.LASSO-003** Resolving the same query twice on the same document state shall return identical index sets.
- **REQ-VIZ.PICK.LASSO-004** When a lasso over 10⁸ points is resolved, the system shall return the index set within 2 s on the reference profile [unverified].
- **REQ-VIZ.PICK.LASSO-005** In visible-only mode, occlusion shall be decided by an f64 CPU depth test against the target geometry, not by reading the GPU depth buffer.
- **REQ-VIZ.PICK.LASSO-006** When a journal containing a lasso query is replayed in re-targeted mode on new data, the query shall re-resolve against the new data deterministically.

## Interfaces

- **Offers:** `view.resolve_lasso@1` (R1); `LassoQuery` type (a GEO selection query).
- **Consumes:** `GEO.SEL` query model; `GEO.INDEX` spatial indexes; f64 camera from view state.

## Decomposition hints

GESTURE (rectangle, polygon, brush to query), PRISM (resolution in f64), VISIBLE (CPU occlusion), PERF (parallel deterministic resolution), RETARGET.

## Open questions

- Whether visible-only mode over 10⁸ points can meet the 2 s budget with a CPU depth test is unmeasured; a fallback that states "through" when the budget is exceeded may be needed.

## Children

_None yet: status `open`._
