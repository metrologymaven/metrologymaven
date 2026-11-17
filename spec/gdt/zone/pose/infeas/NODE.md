---
id: GDT.ZONE.POSE.INFEAS
title: Infeasibility result at a stated size
level: 4
parent: GDT.ZONE.POSE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.POSE-001]
covers: []
depends_on: [GDT.ZONE.POSE.SOLVE, GDT.ZONE.ZTYPE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.POSE.INFEAS: Infeasibility result at a stated size

## Purpose

When a caller asks whether points fit a zone of a stated size (for example a boundary or tier check) rather than for the minimum usage, the engine must say "no" with evidence: the minimum usage found, the pose where it occurs, and the points that lie outside the stated zone at that pose. This leaf wraps `solve_zone` to produce that typed result, so infeasibility is reported uniformly and never as a bare boolean.

## Scope

**In:** `fit_at_size(shape, points, drf, free, size, ...) -> Feasible{solution} | Infeasible{min_usage, pose, violators, cert}`; violator listing at the optimal pose.
**Out:** the solve (owned by `GDT.ZONE.POSE.SOLVE`); deciding conformance from it (owned by UNC through `GDT.CHAR.RECORD`).

## Requirements

- **REQ-GDT.ZONE.POSE.INFEAS-001** When the minimum usage from `solve_zone` is at most the stated size (exact comparison, as ZTYPE `contains`), the module shall return `Feasible` with the solution.
- **REQ-GDT.ZONE.POSE.INFEAS-002** Otherwise it shall return `Infeasible` with the minimum usage, the pose, the certificate, and the indices (ascending) of every point whose per-point zone distance at that pose exceeds half the stated size, with that excess in metres.
- **REQ-GDT.ZONE.POSE.INFEAS-003** If the solve is `LocalOnly`, then an `Infeasible` result shall carry `local_optimum_only`, because a better pose may exist.
- **REQ-GDT.ZONE.POSE.INFEAS-004** If the stated size is not positive or is NaN, then the module shall return `bad_stated_size`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/infeas_planes.json` (minimum usage 0.03 mm, stated 0.02 mm, three points beyond ±0.01 mm at the optimum), when `fit_at_size` runs, then `Infeasible` lists exactly those three indices with their excesses within 1e-12 m.
- **AC-2** Given the same points with stated size 0.03 mm, then `Feasible`.
- **AC-3** Given a solver stub returning `LocalOnly`, then the infeasible result carries `local_optimum_only`.
- **AC-4** (error path) Given stated size 0 or NaN, then `bad_stated_size`.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `Fit { Feasible(ZoneSolution), Infeasible { min_usage_m, pose, cert, violators: Vec<(u32, f64)> } }`.
- **Algorithm:** `solve_zone`; compare; per-point distances at the pose via ZTYPE in index order.
- **Errors:** `bad_stated_size`, pass-through.
- **Determinism:** D0.
- **Performance budget:** solve time plus one linear pass [unverified].
- **Crate:** `fid-gps` (module `zone::pose`).

## Test fixtures

Analytic two-plane and cylinder fixtures with planted violators (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), module `zone::pose`, implement `fit_at_size(shape, points, drf, free, extra, size_m, start, oracle) -> Result<Fit, ZoneError>`: call `solve_zone`; if usage ≤ size (exact) return `Feasible`; otherwise `Infeasible { min_usage_m, pose, cert, violators }` listing ascending indices whose per-point zone distance at the pose exceeds size/2, with the excess; carry `local_optimum_only` when the certificate is local; `bad_stated_size` for size ≤ 0 or NaN. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
