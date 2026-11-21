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
built_in: [fid-gps, fid-gdt]
built_pr: 370
satisfies: [REQ-GDT.DRF-001, REQ-GDT.DRF-002, REQ-GDT.DRF-006, REQ-GDT.DRF-007]
covers: [GOM.GDT.14, PW.GDT.07, SA.GDT.03]
depends_on: [GDT.DRF.DATUM, GDT.DRF.SIM, GDT.RES.RESOLVE]
side_effect: W2
determinism: D0
cli: [fid gdt build-drf, fid gdt get-drf]
mcp: [gdt_build_drf, gdt_get_drf]
journal: [gdt.build_drf@1, gdt.get_drf@1]
updated: 2026-11-20
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

- **Built (this PR, on top of PR #273's pure builder).** `fid_gps::drf::plane_sim` is the measured planar path: each planar datum is simulated from its measured points by `fid-fit`'s tangential plane (`KRN.ENV.TAN.PLANE`) in the variant the resolved `datum_frame/rmb` DMT row's association step names, in precedence order (primary: free normal, certified `strict_local` only; secondary plane after a primary plane: normal held perpendicular to the DRF `z` and free about it; tertiary: normal fixed by the frame, closed form), then assembled by `build_drf`. `fid_gdt::drf::build` holds `gdt.build_drf@1` (W2, dry run R1; label resolution through `GDT.DRF.DATUM`, the chain through `gdt.resolve_chain@1`'s resolver behind the governing-spec gate, the D-045 interpretation block, the `fid.gdt-drf/1` document, its journal event, nothing stored or journaled on a refusal or a dry run), `gdt.get_drf@1` (R0, canonical bytes unchanged) and exact replay with the journal entry's principal bound to each event. Covered: AC-1, AC-3, AC-5, AC-7 (and REQ-001 to REQ-005 for planar datums). Exact oracle: `fixtures/gdt/drf/measured/make_cases.py`.
- **Cylindrical datums are refused, typed (`unsupported_datum_type_p1a{label}`).** `KRN.ENV.MCC.CYL` and `KRN.ENV.TAN.CYL` (the cylindrical datum simulators) are not on main, so AC-2 (plane A, axis B) and AC-4 (single primary axis) are not met through the command; no axis is ever made up. The pure builder already handles simulated axes (PR #273). When a cylinder simulator lands, add an axis path to `plane_sim` (or its successor) and lift the gate.
- **Material boundary.** Only RMB is built (P1a and the D-045 first-review subset). `A(M)` and `A(L)` are refused with `unsupported_material_boundary{label}` before anything else runs; no modifier is read as RMB [unverified for ISO 5459 editions before 2011]. The row must be `datum_frame` with the single modifier `rmb` (`material_boundary_mismatch{row}` otherwise).
- **Overlap with `GDT.DRF.SIM.FIT` (not built).** The planar simulation, including holding the predecessors' DoFs, is that leaf's job; it lives in `plane_sim` here because the build needs it and SIM.FIT is unbuilt. `KRN.ENV.TAN.PLANE` accepts only a free or a fully fixed normal, so the secondary plane (one free rotation about `z`) is solved exactly by enumeration: the optimum of external minimax and constrained L1 over normals perpendicular to `z` lies at a hull-edge normal of the projected points (each objective is concave between those angles), each candidate being `fid-fit`'s closed-form fixed-normal fit. Constrained L2 is refused there (`not_certifiable`) as `fid-fit` refuses it with a free normal. Proposal for SIM.FIT / KRN.ENV.TAN: a "normal perpendicular to `v`" constraint in the KRN fit, after which this enumeration moves there. The hull's orientation test is plain floating point [unverified: no robust predicate].
- **Nominal relationships.** The basic relationship between datums comes from each feature's nominal outward normal (supplied with the measured points, `FeaturePoints`), compared nominal to nominal within `ANGLE_TOL`; oblique datum planes are refused (`oblique_datum_unsupported{label}`). Measured points come from the caller's `FeaturePoints` until a measurement store exists; nominals are recorded as `nominal_outward`, never as datum geometry, and the document says `"values": "measured"`.
- **Pinned fit settings.** `side_tol` 1e-12 m, `rock_tol` 1e-9 and five deterministic starts (no seed) are constants of `plane_sim`; they should come from the DMT row's association parameters once `GDT.RES.SCHEMA` defines them.
- **D-045 label.** The DRF document carries `interpretation`, `interpretation_rules` and `interpretation_register` (`GDT.CHAR.INTERP`), and every surface shows the line from `fid_gdt::requires_provisional_label_bytes`. That predicate signs off only `fid.gdt-result/2` records, so every DRF shows `interpretation: provisional` today even if all its rules were signed off (fail closed). Proposal for `GDT.CHAR.INTERP`: accept `fid.gdt-drf/1` documents in the predicate.
- AC-6 / REQ-006 (ORS-DRF cases) is not met and waits on `UNC.ORS`.
- The DOF sets in REQ-003 are rigid-body geometry, not a dialect interpretation; if an expert finds a dialect-specific exception (for example customized DRFs), it enters as a DMT row option and a later version of this command.

## Children

_Leaf: none._
