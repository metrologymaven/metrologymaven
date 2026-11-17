---
id: GDT.ZONE.MOBSRCH.JOINT
title: Joint zone and datum-mobility solve
level: 4
parent: GDT.ZONE.MOBSRCH
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.MOBSRCH-001, REQ-GDT.ZONE.MOBSRCH-003]
covers: []
depends_on: [GDT.ZONE.POSE.SOLVE, GDT.ZONE.POSE.LIN, GDT.DRF.MOB, GDT.ZONE.MOBSRCH.VERIFY]
uses_artifacts: [UNC.ORS.GDT, UNC.ORS.DRF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.MOBSRCH.JOINT: Joint zone and datum-mobility solve

## Purpose

Places a zone while letting the DRF shift and rotate within its mobility envelope. The DOFs the DRF leaves free are already free in the zone solve; the DOFs it constrains become variables too, bounded by the envelope's rows. One pose vector over the union, the envelope rows appended through the `ExtraConstraints` hook, and the ordinary zone solver do the rest. The DRF part of the returned pose is the transform used, which VERIFY then checks.

## Scope

**In:** translating a `MobilityEnvelope` (linear rows and cones over DRF pose parameters) into `ExtraConstraints` in the LIN variable layout; free set = DRF free DOFs ∪ envelope DOFs; `solve_zone_mobile` for single-zone problems and the same rows offered to the pattern and material-condition solvers; splitting the solved pose into the zone part and the DRF transform used; envelope reference ID and digest on the result.
**Out:** envelope construction (owned by `GDT.DRF.MOB`); membership check (owned by `GDT.ZONE.MOBSRCH.VERIFY`); the solver (owned by `GDT.ZONE.POSE.SOLVE`).

## Requirements

- **REQ-GDT.ZONE.MOBSRCH.JOINT-001** The module shall express each envelope row in the LIN variable layout (one variable per DOF in order tx, ty, tz, rx, ry, rz restricted to the free set) and append it unchanged in meaning to the zone problem.
- **REQ-GDT.ZONE.MOBSRCH.JOINT-002** The free set of the joint problem shall be the DRF free DOFs plus exactly the DOFs the envelope parameterises, and no other.
- **REQ-GDT.ZONE.MOBSRCH.JOINT-003** The result shall report the transform used (the envelope-DOF components of the solved pose, as a rigid transform of the DRF), the envelope reference ID and digest, and the minimised usage recomputed by ZTYPE.
- **REQ-GDT.ZONE.MOBSRCH.JOINT-004** If the envelope is marked non-convex, or the certificate is local only, then the result shall carry `local_optimum_only`.
- **REQ-GDT.ZONE.MOBSRCH.JOINT-005** Every result shall pass through `GDT.ZONE.MOBSRCH.VERIFY` before it is returned.
- **REQ-GDT.ZONE.MOBSRCH.JOINT-006** When the envelope is the single DRF pose (no mobility), the result shall be bit-identical to `solve_zone` on the DRF free DOFs.
- **REQ-GDT.ZONE.MOBSRCH.JOINT-007** The module shall reproduce every ORS-GDT and ORS-DRF mobility case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/mob/hole_mmb.json` (position of a hole to datum A primary plane and datum B a hole at MMB with 0.05 mm radial play, planted toleranced-hole offset 0.08 mm toward the play), when solved, then usage drops from the RMB value by the planted amount within 1e-9 m and the transform used shifts 0.05 mm along the play direction.
- **AC-2** Given an envelope equal to the single DRF pose, then the result is bit-identical to `solve_zone` (REQ-006).
- **AC-3** Given an envelope marked non-convex, then `local_optimum_only` is set.
- **AC-4** Given a VERIFY stub that rejects, then `envelope_violation` and no value.
- **AC-5** (determinism) AC-1 is bit-identical on all CI platforms.
- **AC-6** When ORS-GDT and ORS-DRF mobility cases are published, every case passes within its stated tolerance.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `MobileSolution { zone: ZoneSolution, transform_used: Pose, envelope_ref, envelope_digest }`; uses `MobilityEnvelope` from `GDT.DRF.MOB` and `ExtraConstraints` from LIN.
- **Algorithm:** map envelope rows to the variable layout; call `solve_zone` with the union free set and the rows; split pose; call VERIFY; return.
- **Errors:** `envelope_violation`, `envelope_layout_mismatch`, `zone_solve_failed{code}`.
- **Determinism:** D0 for certified results.
- **Performance budget:** single hole with two mobile datums < 300 ms [unverified].
- **Crate:** `fid-gps` (module `zone::mobility`).

## Test fixtures

Analytic hole-at-MMB fixtures with planted play and offsets (CC0, in-repo); ORS-GDT and ORS-DRF mobility cases read as published artifacts.

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), add module `zone::mobility`. Implement `envelope_rows(envelope: &MobilityEnvelope, layout: &PoseVars) -> Result<ExtraConstraints, MobError>` (error `envelope_layout_mismatch` if the envelope names a parameter outside the layout) and `solve_zone_mobile(shape, points, drf, envelope, oracle) -> Result<MobileSolution, MobError>`: free = DRF free DOFs ∪ envelope DOFs (nothing else); call `zone::pose::solve_zone` with the envelope rows; split the solved pose into the transform used (envelope-DOF components) and the zone part; carry envelope ref and digest; set `local_optimum_only` for non-convex envelopes or local certificates; run `zone::mobility::verify` on every result (`envelope_violation` → no value). With a single-pose envelope the result must be bit-identical to `solve_zone`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- The pattern and material-condition solvers (`GDT.ZONE.GROUP.PATTERN`, `GDT.ZONE.MATL.VCB`) must accept `ExtraConstraints` for mobility to apply to them; both build on the LIN layout, so this is an additive parameter to confirm when those leaves are implemented.

## Children

_Leaf: none._
