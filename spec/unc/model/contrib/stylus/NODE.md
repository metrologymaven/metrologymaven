---
id: UNC.MODEL.CONTRIB.STYLUS
title: Probe and stylus qualification contributor
level: 4
parent: UNC.MODEL.CONTRIB
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.CONTRIB-005]
covers: []
depends_on: [UNC.MODEL.CONTRIB.CONTRACT, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.MODEL.CONTRIB.STYLUS: Probe and stylus qualification contributor

## Purpose

The contributor `uc:stylus-qual@1.0.0`: the uncertainty of the effective stylus tip radius found at qualification, which enters every size result through the radius correction. Its inputs come from the qualification record: the calibrated reference sphere diameter and its expanded uncertainty, the fitted sphere radius standard uncertainty, and the qualification form residual (range).

## Scope

**In:** operator `unc.contrib.stylus_radius@1.0.0` with inputs: reference sphere diameter uncertainty U_ref and k (certificate), fitted radius standard uncertainty u_fit (qualification record), form residual range F (qualification record); output u_r, the standard uncertainty of the effective tip radius: u_r² = (U_ref/(2k))² + u_fit² + (F/(2√3))²; sensitivity to a measured diameter is ±2 (internal −, external +).
**Out:** the qualification itself (owned by `CMM.PROBE`); the contract (owned by `UNC.MODEL.CONTRIB.CONTRACT`).

## Requirements

- **REQ-UNC.MODEL.CONTRIB.STYLUS-001** The operator shall compute u_r as defined in Scope, in metres.
- **REQ-UNC.MODEL.CONTRIB.STYLUS-002** The contributor shall expose sensitivity +2 for external and −2 for internal diameter characteristics and ±1 for radius characteristics, selected by an `orientation` parameter.
- **REQ-UNC.MODEL.CONTRIB.STYLUS-003** The contributor shall accept the reference sphere uncertainty only from `certificate | dcc` sources and u_fit and F only from `qualification` sources.

## Acceptance criteria

- **AC-1** Given U_ref = 0.4 µm, k = 2, u_fit = 0.1 µm, F = 0.6 µm, then u_r = √(0.01 + 0.01 + 0.03)·1e-6 m within 1 ulp.
- **AC-2** Given an internal diameter characteristic, then the expanded sub-graph's sensitivity is −2.
- **AC-3** (error path) Given U_ref from a `declared` source, then `source_kind_not_accepted`.

## Design

- **Registry command(s):** none; kernel operator `unc.contrib.stylus_radius@1.0.0` and data file `uc:stylus-qual@1.0.0`.
- **Errors:** `invalid_input`, `source_kind_not_accepted` (from expansion).
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-kunc` (module `contrib`), data file shipped with `fid-unc`.

## Test fixtures

`fixtures/unc/model/contrib/stylus_cases.json` (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `contrib` (kernel; KNP-1; no panic or `unwrap`), add operator `unc.contrib.stylus_radius@1.0.0`: u_r = √((U_ref/(2k))² + u_fit² + (F/(2√3))²) in metres. Ship contributor `uc:stylus-qual@1.0.0` (`contributor/v1`) with an `orientation` parameter giving sensitivity +2 external diameter, −2 internal diameter, ±1 radius; sources: U_ref from certificate or dcc, u_fit and F from qualification. Tests AC-1..AC-3.

## Open questions

- Treating the form residual as rectangular over its range and combining in quadrature is a conservative project choice that should be reviewed by a probing expert [unverified].

## Children

_Leaf: none._
