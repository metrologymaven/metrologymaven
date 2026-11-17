---
id: LVM.SCALE.DEF
title: Reference-length artefact definitions
level: 3
parent: LVM.SCALE
kind: feature
status: leaf
phase: P3
size: S
owners: []
claimed_by:
satisfies: [REQ-LVM.SCALE-001]
covers: [SA.NET.05]
depends_on: [LVM.STATION.MODEL]
side_effect: W2
determinism: D0
cli: [fid net define-scale-bar, fid net list-scale-bars]
mcp: [net_define_scale_bar, net_list_scale_bars]
journal: [net.define_scale_bar@1]
updated: 2026-11-16
---

# LVM.SCALE.DEF: Reference-length artefact definitions

## Purpose

Records each calibrated reference length the network can use: a scale bar or a calibrated distance between two targets, with its calibrated value, calibration uncertainty, thermal behaviour and certificate reference. Definitions are immutable once created; a re-issued certificate is a new definition, so every adjustment can name exactly which calibration it used.

## Scope

**In:**
- `ReferenceArtefact` with kind (`scale_bar` or `reference_distance`), label, calibrated length, expanded uncertainty and coverage factor, reference temperature, thermal expansion coefficient with optional standard uncertainty, certificate ID and optional end-target names
- Validation of the definition
- Listing definitions

**Out** (and which node owns it instead):
- Binding to common points and constraint supply (`LVM.SCALE.BIND`)
- Thermal correction (`LVM.SCALE.THERM`)
- Certificate signing or verification (`TRUST`)

## Requirements

- **REQ-LVM.SCALE.DEF-001** When `net.define_scale_bar@1` runs, the system shall store a reference artefact with a UUIDv7 ID, unique label, kind, calibrated length (authored value and unit kept, SI value in m), optional expanded uncertainty U with coverage factor k, reference temperature (authored value kept, SI value in K), thermal expansion coefficient in 1/K with optional standard uncertainty, and certificate ID as untrusted text.
- **REQ-LVM.SCALE.DEF-002** If the length is not finite and positive, U is negative or non-finite, k is given without U or is not positive, or the expansion coefficient is non-finite, then the command shall fail with `NET_SCALE_DEF_INVALID` naming the field and journal nothing.
- **REQ-LVM.SCALE.DEF-003** The system shall offer no command that changes a stored definition; a new calibration shall be a new definition.
- **REQ-LVM.SCALE.DEF-004** When `net.list_scale_bars@1` runs, the system shall list definitions in creation order with all fields and the number of bindings that use each.

## Acceptance criteria

- **AC-1** Given an empty network from fixture `fixtures/lvm/net-small-3st`, when `fid net define-scale-bar --label SB1 --length "1500.0213 mm" --U "0.006 mm" --k 2 --t-ref "20 degC" --alpha 2.0e-6 --cert "CERT-0042" --json` runs, then `fid net list-scale-bars --json` shows SB1 with length 1.5000213 m, U 6e-6 m, k 2, T_ref 293.15 K, the authored strings and 0 bindings.
- **AC-2** (error path) Given `--length "-1 m"`, when the command runs, then it fails with `NET_SCALE_DEF_INVALID` naming `length` and the journal length is unchanged.
- **AC-3** (error path) Given `--k 2` without `--U`, then it fails with `NET_SCALE_DEF_INVALID` naming `k`.
- **AC-4** (no uncertainty) Given a definition without `--U`, then it is stored with `uncertainty: none`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `net.define_scale_bar@1` (W2): `network, label, kind, length: Quantity, expanded_u: Option<Quantity>, k: Option<f64>, t_ref: Quantity, alpha: f64, u_alpha: Option<f64>, certificate: UntrustedText, end_names: Option<[String; 2]>` → `ArtefactId`; `net.list_scale_bars@1` (R0).
- **Data structures:** `ReferenceArtefact { id, label, kind, length_m, length_authored, u_expanded_m: Option<f64>, k: Option<f64>, t_ref_k, t_ref_authored, alpha_per_k, u_alpha: Option<f64>, certificate, end_names }`.
- **Conversion:** one journaled rounding per converted value (mm to m, °C to K) through the shared units facility.
- **Errors:** `NET_SCALE_DEF_INVALID { field }`, `NET_DUPLICATE_LABEL`.
- **Determinism:** D0 (exact parse plus one multiplication or addition per conversion).
- **Crate:** `fid-lvm` (proposed name).

## Test fixtures

- `fixtures/lvm/net-small-3st` (shared), CC-BY-4.0; invalid-definition cases written inline in the tests.

## Build prompt

> In the crate `fid-lvm`, add reference-length artefact definitions for large-volume networks. Register `net.define_scale_bar@1` (W2) and `net.list_scale_bars@1` (R0) with CLI `fid net define-scale-bar|list-scale-bars` and `--json`; MCP names are generated. A definition has a UUIDv7 ID, unique label, kind (`scale_bar` or `reference_distance`), calibrated length (authored text and SI metres), optional expanded uncertainty U and coverage factor k, reference temperature (authored text and kelvin), thermal expansion coefficient (1/K) with optional standard uncertainty, a certificate ID stored as untrusted text (CONSTITUTION C6) and optional end-target names. Validation fails with `NET_SCALE_DEF_INVALID { field }` and journals nothing for: non-finite or non-positive length, negative or non-finite U, k without U or k ≤ 0, non-finite alpha. Definitions are immutable: provide no update command. Unit conversions are one journaled rounding each (CONSTITUTION §4). No panics, no `unwrap`, no `unsafe` (CONSTITUTION §8). Tests: the five acceptance criteria above. Determinism class D0.

## Open questions

- None yet.

## Children

_Leaf: none._
