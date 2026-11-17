---
id: LVM.SCALE.BIND
title: Reference-length binding and scale constraints
level: 3
parent: LVM.SCALE
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-LVM.SCALE-003, REQ-LVM.SCALE-005]
covers: []
depends_on: [LVM.SCALE.DEF, LVM.STATION.MODEL, LVM.ADJ.OBSEQ]
side_effect: W2
determinism: D0
cli: [fid net bind-reference-length, fid net set-reference-length-enabled, fid net list-reference-lengths]
mcp: [net_bind_reference_length, net_set_reference_length_enabled, net_list_reference_lengths]
journal: [net.bind_reference_length@1, net.set_reference_length_enabled@1]
updated: 2026-11-16
---

# LVM.SCALE.BIND: Reference-length binding and scale constraints

## Purpose

Ties a defined reference artefact to the two common points that mark its ends in one placement, and turns each binding into a weighted distance constraint for the adjustment. A scale bar measured in several orientations is one artefact with several bindings. Bindings without calibration uncertainty may only be check lengths, never weighted constraints.

## Scope

**In:**
- `ReferenceBinding`: artefact, common points A and B, mode (`constraint` or `check`), enabled flag, optional thermal correction slot (filled by `LVM.SCALE.THERM`)
- Validation of bindings
- Building the list of `DistanceConstraint`s with standard deviations for `LVM.ADJ`

**Out** (and which node owns it instead):
- Artefact definitions (`LVM.SCALE.DEF`)
- Thermal correction values (`LVM.SCALE.THERM`)
- Distance observation equation (`LVM.ADJ.OBSEQ`)
- Post-adjustment scale check (`LVM.SCALE.CHECK`)

## Requirements

- **REQ-LVM.SCALE.BIND-001** When `net.bind_reference_length@1` runs, the system shall store a binding with a UUIDv7 ID, the artefact ID, two distinct existing common points and a mode of `constraint` or `check`.
- **REQ-LVM.SCALE.BIND-002** If the mode is `constraint` and the artefact has no expanded uncertainty, then the command shall fail with `NET_SCALE_NO_UNCERTAINTY` and journal nothing; mode `check` shall be accepted.
- **REQ-LVM.SCALE.BIND-003** If the two common points are the same, unknown or merged, then the command shall fail with `NET_SCALE_BIND_INVALID`.
- **REQ-LVM.SCALE.BIND-004** The system shall supply one `DistanceConstraint` per enabled binding in mode `constraint`, in binding creation order, whose value is the thermally corrected length when one is set and the calibrated length otherwise, and whose standard deviation is √((U/k)² + u_th²), with u_th the thermal-correction standard uncertainty or 0.
- **REQ-LVM.SCALE.BIND-005** When `net.set_reference_length_enabled@1` runs, the system shall change only the enabled flag and journal one event.
- **REQ-LVM.SCALE.BIND-006** Bindings of the same artefact shall be supplied as independent constraints, and the constraint list shall state this assumption in its metadata.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/net-scalebar-2pos` (one artefact SB1 with U = 6e-6 m, k = 2, observed in 2 placements as points SB1A/SB1B and SB2A/SB2B), when both placements are bound in mode `constraint`, then the constraint list has 2 entries in creation order with value 1.5000213 m and standard deviation 3e-6 m within 1e-18.
- **AC-2** (error path) Given an artefact without U, when it is bound in mode `constraint`, then the command fails with `NET_SCALE_NO_UNCERTAINTY` and the journal length is unchanged; in mode `check` it succeeds and the constraint list stays empty.
- **AC-3** (error path) Given points A = B, then binding fails with `NET_SCALE_BIND_INVALID`.
- **AC-4** (thermal) Given a binding with a thermal correction of value 1.5000243 m and u_th = 1e-6 m set by `LVM.SCALE.THERM`, then its constraint value is 1.5000243 m and its standard deviation is √(9e-12 + 1e-12) m.
- **AC-5** (enable) Given one binding disabled, then the constraint list omits it.
- **AC-6** (replay) Given the journal from AC-1 to AC-5, when replayed in exact mode, then the outcome is `IDENTICAL` and the constraint list digests match.

## Design

- **Registry commands:** `net.bind_reference_length@1` (W2): `artefact: ArtefactId, a: PointRef, b: PointRef, mode: Constraint|Check` → `BindingId`; `net.set_reference_length_enabled@1` (W2); `net.list_reference_lengths@1` (R0).
- **Data structures:** `ReferenceBinding { id, artefact, a, b, mode, enabled, thermal: Option<ThermalCorrection { value_m, u_m, source_event }> }`; internal API `fn scale_constraints(net: &Network) -> ConstraintList` returning `DistanceConstraint { a, b, value_m, sigma_m, binding }` as defined by `LVM.ADJ.OBSEQ`.
- **Algorithm:** σ = √((U/k)² + u_th²) evaluated in that fixed order with `fid-num` sqrt; no FMA.
- **Errors:** `NET_SCALE_NO_UNCERTAINTY`, `NET_SCALE_BIND_INVALID`, `NET_UNKNOWN_ARTEFACT`.
- **Determinism:** D0 (IEEE sqrt is correctly rounded).
- **Crate:** `fid-lvm` (proposed name).

## Test fixtures

- `fixtures/lvm/net-scalebar-2pos`: seeded synthetic generator (seed 4), 2 stations, 1 scale bar in 2 placements; CC-BY-4.0.

## Build prompt

> In the crate `fid-lvm`, add reference-length bindings. Register `net.bind_reference_length@1 (artefact, a, b, mode)` (W2), `net.set_reference_length_enabled@1` (W2) and `net.list_reference_lengths@1` (R0), with CLI `fid net bind-reference-length|set-reference-length-enabled|list-reference-lengths` and `--json`; MCP names are generated. A binding links an existing reference artefact (from `net.define_scale_bar@1`) to two distinct, existing, unmerged common points, in mode `constraint` or `check`. Mode `constraint` requires the artefact to have an expanded uncertainty, else fail with `NET_SCALE_NO_UNCERTAINTY`; bad points fail with `NET_SCALE_BIND_INVALID`; failures journal nothing. Provide `scale_constraints(&Network)` returning one `DistanceConstraint { a, b, value_m, sigma_m, binding }` (type from the kernel crate `fid-lvm-adj`) per enabled constraint-mode binding, in creation order, with value = thermally corrected length if present else calibrated length, and sigma = √((U/k)² + u_th²) with u_th = 0 when absent. Bindings of one artefact are independent constraints; record that assumption in the list metadata. No FMA, no std HashMap, no panics, no `unwrap`, no `unsafe` (CONSTITUTION §4, §8). Tests: the six acceptance criteria above with a seeded fixture generator. Determinism class D0.

## Open questions

- Several bindings of one artefact share its calibration error, so they are correlated. Treating them as independent is a simplification that understates the scale uncertainty [unverified]; a reviewer should decide whether to model the shared term.

## Children

_Leaf: none._
