---
id: SCAN.CMP.GPUPV.PVFIELD
title: PreviewField type and preview marker
level: 4
parent: SCAN.CMP.GPUPV
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP.GPUPV-001, REQ-SCAN.CMP.GPUPV-002]
covers: []
depends_on: [SCAN.CMP.DEV.PRIM]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.CMP.GPUPV.PVFIELD: PreviewField type and preview marker

## Purpose

Makes C2 enforceable by construction for deviation previews: GPU-computed values live only in `PreviewField`, which sits in a separate L3 crate that the value-of-record crate `fid-scan-cmp` cannot see, has no conversion into `DeviationField`, a report value or a ResultRecord, and always serialises with `preview: true`.

## Scope

**In:**
- Crate `fid-scan-preview` (L3, display-only) and its `PreviewField` type: per-point f32 or f64 value, status, the `DevParams` it approximates, and the source digests
- Serialisation with mandatory `preview: true` and a display badge string key
- Compile-fail tests proving no conversion path exists

**Out** (and which node owns it instead):
- Computing preview values on the GPU (owned by `SCAN.CMP.GPUPV.PVGPU`)
- `DeviationField` (owned by `SCAN.CMP.DEV.FIELD`)

## Requirements

- **REQ-SCAN.CMP.GPUPV.PVFIELD-001** `PreviewField` shall be defined in crate `fid-scan-preview`, and no crate in the kernel or L2 layers shall depend on that crate, checked by the layer and dependency-graph lint.
- **REQ-SCAN.CMP.GPUPV.PVFIELD-002** `PreviewField` shall implement no `From`, `Into`, `TryFrom` or other conversion into `DeviationField`, `FieldBuilder`, report value types or ResultRecord, proven by compile-fail tests.
- **REQ-SCAN.CMP.GPUPV.PVFIELD-003** Every serialised form of a `PreviewField` (JSON for CLI and MCP, in-process handle for the GUI) shall carry `preview: true` and the badge key `scan.cmp.preview_badge`, and deserialising a payload without `preview: true` shall fail.

## Acceptance criteria

- **AC-1** Given compile-fail tests that try `DeviationField::from(preview)`, `preview.into()` for report value and ResultRecord types, compilation fails.
- **AC-2** Given the workspace dependency graph, no crate outside L3 depends on `fid-scan-preview` (lint passes; a seeded violation fails).
- **AC-3** Given a serialised preview, the JSON contains `"preview": true`; removing it makes deserialisation fail with `scan.cmp.preview_marker_missing`.

## Design

- **API** (`fid-scan-preview::field`): `pub struct PreviewField { values: Vec<f32>, status: Vec<DevStatus>, params: DevParams, actual_digest, nominal_digest }` with read accessors only; `DevStatus` and `DevParams` re-used from `fid-scan-cmp` (one-way dependency L3 → L2).
- **Errors:** `scan.cmp.preview_marker_missing`.
- **Determinism:** N (GPU values are observed, not reproducible bit-for-bit).
- **Crate:** `fid-scan-preview` (layer L3, display-only).

## Test fixtures

None beyond compile-fail tests and a synthetic 10-point preview.

## Build prompt

> Create the Rust crate `fid-scan-preview` (layer L3, display-only) with module `field`: `PreviewField { values: Vec<f32>, status, params: DevParams, actual_digest, nominal_digest }`, read accessors only, serde with a mandatory `preview: true` field and badge key `scan.cmp.preview_badge`; deserialisation without the marker fails `scan.cmp.preview_marker_missing`. It may depend on `fid-scan-cmp` for `DevStatus` and `DevParams`; nothing in L0–L2 may depend on it (add the rule to the layer lint). Implement no conversion into `DeviationField`, `FieldBuilder`, report values or ResultRecord, and add trybuild compile-fail tests for each.
>
> Constraints: CONSTITUTION C2, §2.1 layers, §8 (no panic/unwrap/unsafe). Tests: AC-1..AC-3.

## Open questions

- The crate is placed in L3 because it needs the wgpu device from VIZ; confirm the layer placement at review.

## Children

_Leaf: none._
