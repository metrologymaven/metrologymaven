---
id: GDT.DRF.BUILD
title: DRF construction, precedence and DOF bookkeeping
level: 3
parent: GDT.DRF
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF-001, REQ-GDT.DRF-002, REQ-GDT.DRF-006, REQ-GDT.DRF-007]
covers: [GOM.GDT.14, PW.GDT.07, SA.GDT.03]
depends_on: [GDT.DRF.DATUM, GDT.DRF.SIM, GDT.RES.RESOLVE]
side_effect: W2
determinism: D0
cli: [fid gdt build-drf, fid gdt get-drf]
mcp: [gdt_build_drf, gdt_get_drf]
journal: [gdt.build_drf@1, gdt.get_drf@1]
updated: 2026-11-21
---

# GDT.DRF.BUILD: DRF construction, precedence and DOF bookkeeping

## Purpose

Builds a datum reference frame from one to three simulated datums in the precedence order of the feature control frame, records which of the six rigid-body degrees of freedom each datum constrains, reports the ones left free, and rejects degenerate combinations. In P1a it handles planar and cylindrical datum features at RMB; the simulated datum (plane or axis) comes from `GDT.DRF.SIM` through a narrow interface, so this leaf is pure geometry and bookkeeping.

## Scope

**In:** precedence (primary, secondary, tertiary) for simulated planes and axes; DOF table per datum and remaining free DOFs, classified by ISO 17450-1 invariance class; DRF origin and axes from the simulated datums; degeneracy detection; `gdt.build_drf@1` (W2) and `gdt.get_drf@1` (R0).
**Out:** datum-feature simulators and rocking (owned by `GDT.DRF.SIM`); common datums and datum targets (owned by `GDT.DRF.COMMON`); mobility (owned by `GDT.DRF.MOB`); containment flags (owned by `GDT.DRF.CHECK`); alignments (owned by `ALN`).

## Requirements

- **REQ-GDT.DRF.BUILD-001** The system shall build a DRF from one to three simulated datums in the stated precedence, each datum constraining only DOFs left free by the datums before it.
- **REQ-GDT.DRF.BUILD-002** For each datum the DRF shall record the constrained DOFs as a subset of {tx, ty, tz, rx, ry, rz} in the DRF's own axes, and the DRF shall list the remaining free DOFs.
- **REQ-GDT.DRF.BUILD-003** A primary simulated plane shall constrain one translation and two rotations; a primary simulated axis shall constrain two translations and two rotations; a secondary and tertiary datum shall constrain the DOFs its invariance class removes from those still free.
- **REQ-GDT.DRF.BUILD-004** If a datum constrains no DOF left free by its predecessors (for example a secondary plane parallel to the primary plane within 1e-9 rad), then the system shall reject the DRF with `degenerate_datum_pair` naming the pair.
- **REQ-GDT.DRF.BUILD-005** The DRF shall record the simulator algorithm ID used for each datum and the DMT row that named it.
- **REQ-GDT.DRF.BUILD-006** DRF construction shall match every ORS-DRF case within the P1a scope (planar and cylindrical, RMB) within the case's stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/block_abc.json` (three simulated mutually perpendicular planes), when `fid gdt build-drf --datums A,B,C --json` runs, then A constrains {tz, rx, ry}, B {ty, rz}, C {tx}, free DOFs is empty, and the origin and axes match the analytic intersection within 1e-12 m and 1e-12 rad.
- **AC-2** Given fixture `plate_bore.json` (primary plane A, secondary axis B perpendicular to A), then A constrains {tz, rx, ry}, B {tx, ty}, and the free DOF list is {rz}.
- **AC-3** (error path) Given B parallel to A, then the build fails with `degenerate_datum_pair{A,B}` and journals nothing.
- **AC-4** Given a single primary axis, then free DOFs are {tz, rz} (translation and rotation along the axis).
- **AC-5** Given the committed DRF of AC-1, when `fid gdt get-drf --id <drf> --json` runs, then the canonical JSON is returned unchanged.
- **AC-6** (ORS) Every ORS-DRF case tagged `p1a` passes within its tolerance (runs when `UNC.ORS` publishes them).
- **AC-7** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.build_drf@1` (W2; dry-run R1; params `char: CharId`, `datums: [DatumLabel; 1..=3]`; result `Drf`); `gdt.get_drf@1` (R0; params `id`).
- **Data structures:** `SimulatedDatum { Plane{point, normal} | Axis{point, direction} }` (from SIM), `Drf { origin, axes: [unit; 3], dof: [DatumDof], free: DofSet, simulators: [AlgId], dmt_row }`, `DofSet` as a 6-bit set.
- **Algorithm:** primary sets the first axis (plane normal or axis direction) and constrained DOFs; secondary is projected orthogonally to remove remaining rotation and translation; tertiary completes; Gram–Schmidt in fixed order; degeneracy when the projected component's norm < 1e-9.
- **Errors:** `degenerate_datum_pair{a,b}`, `too_many_datums`, `datum_undefined{label}`, `unsupported_datum_type_p1a`, pass-through simulator errors.
- **Determinism:** D0 (closed-form, KNP-1).
- **Performance budget:** < 20 µs per DRF excluding simulation [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Synthetic simulated-datum sets (analytic planes and axes) generated in-repo, CC0; ORS-DRF cases from `UNC.ORS` once published.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), implement DRF construction from one to three `SimulatedDatum` values (`Plane{point, normal}` or `Axis{point, direction}`) in precedence order. Record per datum the constrained DOFs as a 6-bit set over {tx, ty, tz, rx, ry, rz} in DRF axes: a primary plane removes {tz, rx, ry}; a primary axis removes {tx, ty, rx, ry}; secondary and tertiary datums remove what their invariance class removes from the still-free set; list remaining free DOFs. Build orthonormal axes by fixed-order Gram–Schmidt; reject `degenerate_datum_pair{a,b}` when a datum removes nothing new (projected norm < 1e-9). Record simulator algorithm IDs and the DMT row. Register `gdt.build_drf@1` (W2, dry-run R1; CLI `fid gdt build-drf`; MCP `gdt_build_drf`) and `gdt.get_drf@1` (R0; CLI `fid gdt get-drf`; MCP `gdt_get_drf`). Tests: acceptance criteria above. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` for transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- **Partial build (PR #370 planar, then the cylindrical path; not marked `built_in`).** Open AC: **AC-6** only (waits on `UNC.ORS`); AC-1 to AC-5 and AC-7 are met (AC-2 for an external cylinder, below). Built: `fid_gps::drf::cyl_sim::build_measured_drf` simulates each datum from its measured points in precedence order: planes by `fid-fit`'s tangential plane (`KRN.ENV.TAN.PLANE`) as `plane_sim` does (primary: free normal, certified `strict_local` only; after a free `rz`: normal fixed at `+-z` or held perpendicular to `z` and free about it, by enumeration; with every rotation fixed: normal fixed by the frame), external cylinders by the minimum circumscribed cylinder (`KRN.ENV.MCC.CYL`, `fit.cylinder.mc@1.0.0`). `fid_gdt::drf::build` holds `gdt.build_drf@1` (W2, dry run R1; label resolution through `GDT.DRF.DATUM`, the chain through `gdt.resolve_chain@1`'s resolver behind the governing-spec gate, the D-045 interpretation block, the `fid.gdt-drf/1` document, its journal event, nothing stored or journaled on a refusal or a dry run), `gdt.get_drf@1` (R0, canonical bytes unchanged) and exact replay, which binds each event to its journal entry's principal and re-runs it through the register its pin names (loaded by digest, re-checked for `Purpose::Replay`), for planar and cylindrical DRFs alike. Covered: AC-1 to AC-5, AC-7, REQ-001 to REQ-005. Exact oracles (stdlib Python): `fixtures/gdt/drf/measured/make_cases.py` (planes) and `fixtures/gdt/drf/cyl/make_cases.py` (planted five-contact envelopes recomputed by Decimal Newton on the rounded points, fixed-direction envelopes as exact smallest enclosing circles of the projections, frames per case; DoF sets hand-labelled from REQ-003), including seeded precedence faults with cylinders (`ab` against `ba`, `abc` against `acb` and `bac`).
- **Cylinder simulator and its gate.** Every envelope goes through `fid-fit`'s `to_fit_result`, the `Fit` trait's own fail-closed gate (`MccCylinderFit::fit` is `fit_mcc_cylinder` followed by it), and is re-checked: a free direction must be a certified strict local minimum whose global cross-check `passed`, a fixed direction the certified global minimum of the convex problem, and no flag (`local_optimum_only`, `suspect_axis`, the certificate's flag) is accepted; anything else is `flagged_fit{label}`. Other `fid-fit` refusals pass through as `simulation_failed{label:code}`; an oval cross-section (a degenerate active set, which `KRN.ENV.MCC.CYL` refuses) is `simulation_failed{label:solver}` (tested as primary and as a secondary). The axis is the fit's axis through the foot of the points' centroid, signed by the nominal axis. Seed pinned at 0 and recorded (`fit.seed`); the free fit's cross-check outcome is recorded (`fit.cross_check`).
- **Orientation of an axis datum (pinned).** The constraint comes from the free DoFs and the nominal directions, as for planes: a primary cylinder has a free direction; after a primary plane or axis (only `rz` free) a cylinder whose nominal axis is parallel to the primary's nominal is held at `+-z` (fixed direction); with every rotation fixed it is held at the measured `z`, or, after a secondary plane, at the measured `y` or `x`. A cylinder whose nominal axis is perpendicular to `z` while `rz` is free (a cross hole or a horizontal shaft on a primary plane) would need its axis held perpendicular to `z` and free about it, which `KRN.ENV.MCC.CYL` cannot do (free or fixed direction only): refused, `orientation_constraint_unsupported{label}`. Proposal for `KRN.ENV.MCC.CYL` / `GDT.DRF.SIM.FIT`: an "axis perpendicular to `v`" constraint. Oblique nominals are refused (`oblique_datum_unsupported{label}`).
- **Internal cylinders (bores) are refused, typed.** Their simulator is the maximum inscribed cylinder (`KRN.ENV.MIC.CYL`), which is not built, so a cylinder measured with side `internal` is `unsupported_datum_type_p1a{label}` (`feature_type` `internal_cylinder`) before anything is fitted; no axis is ever made up. AC-2 names a fixture `plate_bore.json`; it is met with an external boss perpendicular to plane A (case `ab`: A {tz, rx, ry}, B {tx, ty}, free {rz}), whose DoF table is the same as a bore's. When `KRN.ENV.MIC.CYL` lands, add it as a third simulator of the chain and lift the gate; the bore variant of AC-2 then needs its own reference case.
- **Chain and inputs.** The `datum_frame/rmb` row's chain may name at most one tangential plane and at most one `fit.cylinder.mc@1.0.0` association step (`unsupported_simulator{alg}` otherwise; `unsupported_simulator{none}` when a datum's kind has no simulator in the chain). A cylinder's measured points come with its nominal axis direction and side (`FeaturePoints::insert_cylinder`; the digest domain differs from a plane's), and points measured as the other kind of feature are `measurement_invalid{feature:kind_mismatch}`. The document's cylinder rows carry `axis`, `radius`, `nominal_axis` and `side` instead of `plane` and `nominal_outward`; planar documents are byte-for-byte unchanged. `fid-fit` has no output digest for the envelope yet, so `fit.output_digest` of a cylinder is this crate's SHA-256 over the axis, radius, contacts and certificate digest (`fid.gdt-drf-cylinder-fit/1`).
- **Cost.** A free-direction envelope runs `fid-fit`'s global cross-check (measured there at 0.1 to 1.2 s release on 1 000 to 2 000 points); the 20 us budget excludes simulation [unverified for large point sets].
- **Material boundary.** Only RMB is built (P1a and the D-045 first-review subset). `A(M)` and `A(L)` are refused with `unsupported_material_boundary{label}` before anything else runs; no modifier is read as RMB [unverified for ISO 5459 editions before 2011]. The row must be `datum_frame` with the single modifier `rmb` (`material_boundary_mismatch{row}` otherwise).
- **Overlap with `GDT.DRF.SIM.FIT` (not built).** The planar simulation, including holding the predecessors' DoFs, is that leaf's job; it lives in `plane_sim` here because the build needs it and SIM.FIT is unbuilt. `KRN.ENV.TAN.PLANE` accepts only a free or a fully fixed normal, so the secondary plane (one free rotation about `z`) is solved exactly by enumeration (a selected normal more than 60 degrees from the nominal is refused, `off_nominal{label}`): the optimum of external minimax and constrained L1 over normals perpendicular to `z` lies at a hull-edge normal of the projected points (each objective is concave between those angles), each candidate being `fid-fit`'s closed-form fixed-normal fit. Constrained L2 is refused there (`not_certifiable`) as `fid-fit` refuses it with a free normal. Proposal for SIM.FIT / KRN.ENV.TAN: a "normal perpendicular to `v`" constraint in the KRN fit, after which this enumeration moves there. The hull's orientation test is plain floating point [unverified: no robust predicate].
- **Nominal relationships.** The basic relationship between datums comes from each feature's nominal outward normal (supplied with the measured points, `FeaturePoints`), compared nominal to nominal within `ANGLE_TOL`; oblique datum planes are refused (`oblique_datum_unsupported{label}`). Measured points come from the caller's `FeaturePoints` until a measurement store exists; nominals are recorded as `nominal_outward`, never as datum geometry, and the document says `"values": "measured"`.
- **Pinned fit settings.** `side_tol` 1e-12 m, `rock_tol` 1e-9 and five deterministic starts (no seed) are constants of `plane_sim`; they should come from the DMT row's association parameters once `GDT.RES.SCHEMA` defines them.
- **D-045 label.** The DRF document carries `interpretation`, `interpretation_rules` and `interpretation_register` (`GDT.CHAR.INTERP`), and every surface shows the line from `fid_gdt::requires_provisional_label_bytes`. That predicate signs off only `fid.gdt-result/2` records, so every DRF shows `interpretation: provisional` today even if all its rules were signed off (fail closed). Proposal for `GDT.CHAR.INTERP`: accept `fid.gdt-drf/1` documents in the predicate.
- AC-6 / REQ-006 (ORS-DRF cases, planar and cylindrical) is not met and waits on `UNC.ORS`; it is the only open AC.
- The DOF sets in REQ-003 are rigid-body geometry, not a dialect interpretation; if an expert finds a dialect-specific exception (for example customized DRFs), it enters as a DMT row option and a later version of this command.

## Children

_Leaf: none._
