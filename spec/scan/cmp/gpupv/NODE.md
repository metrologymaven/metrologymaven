---
id: SCAN.CMP.GPUPV
title: GPU deviation preview with CPU cross-check
level: 3
parent: SCAN.CMP
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP-004, REQ-SCAN.CMP-005]
covers: []
depends_on: [SCAN.CMP.DEV, VIZ.RENDER]
cli: [fid compare surface-preview]
mcp: [compare_surface_preview]
journal: []
updated: 2026-11-16
---

# SCAN.CMP.GPUPV: GPU deviation preview with CPU cross-check

## Purpose

An optional fast preview of a surface comparison computed on the GPU, so that users can see an approximate colour map while parameters change, before the CPU kernel produces the value of record. It exists to make the C2 rule enforceable: preview values are typed and labelled as preview, can never enter a deviation field, report or ResultRecord, and are tested against the CPU result on reference fixtures.

## Scope

**In:**
- GPU compute of approximate per-point deviations against a tessellated nominal
- `PreviewField` type that is distinct from `DeviationField` and cannot be converted into it
- Preview labelling in every output (UI badge source, MCP output schema field `preview: true`)
- Cross-check harness: GPU preview versus CPU field on reference fixtures with a published tolerance

**Out** (and which node owns it instead):
- The value-of-record computation (owned by `SCAN.CMP.DEV`)
- Drawing the preview (owned by `VIZ.OVERLAY` and `VIZ.RENDER`)
- GPU backend support matrix (owned by `PLAT`)

## Requirements

- **REQ-SCAN.CMP.GPUPV-001** Every GPU-computed deviation shall be held only in a `PreviewField` type that has no conversion into a `DeviationField`, report value or ResultRecord.
- **REQ-SCAN.CMP.GPUPV-002** Every output of the preview command, in every surface (GUI, CLI, MCP), shall carry an explicit preview marker.
- **REQ-SCAN.CMP.GPUPV-003** On each reference fixture, the preview shall agree with the CPU deviation field within a published tolerance, initially 1 µm or 1 % of the value, whichever is larger, for points that are `VALID` in both [unverified].
- **REQ-SCAN.CMP.GPUPV-004** The preview command shall be class R1 and shall not be journaled as a value-producing command.
- **REQ-SCAN.CMP.GPUPV-005** If no supported GPU is available, then the preview command shall return a typed `preview_unavailable` result and the CPU path shall remain fully functional.

## Interfaces

- **Offers:** `compare.surface_preview@1` (R1); `PreviewField`; cross-check report used by CI.
- **Consumes:** SCAN.CMP.DEV (CPU reference for the cross-check and the parameter set), VIZ.RENDER (wgpu device and buffers).

## Decomposition hints

PVFIELD (preview type and its non-convertibility), PVGPU (the compute shader path), XCHECK (CPU-versus-GPU harness and published tolerance).

## Open questions

- Whether a preview is needed in P1b at all, or can move to a later phase once CPU timings are measured; the phase is kept at P1b pending those timings.

## Children

_None yet: status `open`._
