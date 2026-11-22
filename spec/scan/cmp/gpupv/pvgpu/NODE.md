---
id: SCAN.CMP.GPUPV.PVGPU
title: GPU deviation preview command
level: 4
parent: SCAN.CMP.GPUPV
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP.GPUPV-002, REQ-SCAN.CMP.GPUPV-004, REQ-SCAN.CMP.GPUPV-005]
covers: []
depends_on: [SCAN.CMP.GPUPV.PVFIELD, SCAN.CMP.DEV.PRIM, VIZ.RENDER.FRAME, GEO.INDEX.BVH]
side_effect: R1
determinism: N
cli: [fid compare surface-preview]
mcp: [compare_surface_preview]
journal: [compare.surface_preview@1]
updated: 2026-11-22
---

# SCAN.CMP.GPUPV.PVGPU: GPU deviation preview command

## Purpose

Computes an approximate deviation map on the GPU against a tessellated nominal so that the colour map can follow parameter changes interactively. It is an R1 command: its result is a `PreviewField`, it writes nothing to the Record Journal (Activity Log only, per CONSTITUTION §3), and when no GPU is available it says so and leaves the CPU path untouched.

## Scope

**In:**
- `compare.surface_preview@1` (R1): actual (cloud, mesh or a display LOD subset), nominal (tessellated CAD or mesh), method, search distance, angle cutoff, alignment
- WGSL compute pass: BVH traversal over nominal triangles uploaded from the CPU-built BVH, closest point, sign by face normal, angle cutoff
- `preview_unavailable` result when no adapter with compute support exists

**Out** (and which node owns it instead):
- The preview type (owned by `SCAN.CMP.GPUPV.PVFIELD`)
- Drawing the preview (owned by `VIZ.OVERLAY` and `VIZ.RENDER`)
- Cross-check against the CPU (owned by `SCAN.CMP.GPUPV.XCHECK`)

## Requirements

- **REQ-SCAN.CMP.GPUPV.PVGPU-001** `compare.surface_preview@1` shall be class R1, MCP exposed, write no Record Journal entry, and return only a `PreviewField` marked `preview: true`.
- **REQ-SCAN.CMP.GPUPV.PVGPU-002** The preview shall accept the same method, search distance, angle cutoff and alignment parameters as `compare.surface_cad@1` and apply the same `NO_MATCH` rules approximately on the tessellated nominal.
- **REQ-SCAN.CMP.GPUPV.PVGPU-003** If no wgpu adapter with compute shaders is available, or the device is lost during the run, then the command shall return the typed result `preview_unavailable` with a reason, and shall not fail any CPU command.
- **REQ-SCAN.CMP.GPUPV.PVGPU-004** A preview of 10⁷ points against 10⁶ nominal triangles shall complete within 2 s on reference hardware [unverified].
- **REQ-SCAN.CMP.GPUPV.PVGPU-005** During interactive viewport scrubbing, the preview compute pass shall bind its output storage buffer directly to the display shading pass with zero CPU readback and an active `PREVIEW — NOT VALUE OF RECORD` HUD indicator, performing CPU `PreviewField` readback only when explicitly requested by CLI/MCP or cross-check (`SCAN.CMP.GPUPV.XCHECK`).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cmp/panel_scan` on a machine with a supported GPU, when `fid compare surface-preview ... --json` runs, then the output has `preview: true`, one status per point, and the Record Journal length is unchanged.
- **AC-2** Given the wgpu software adapter disabled and no hardware adapter, the command returns `preview_unavailable { reason: "no_adapter" }` and `compare.surface_cad@1` still succeeds.
- **AC-3** Given an MCP call, the tool output schema contains the required `preview` boolean.
- **AC-4** (bench, advisory) REQ-004 is measured and published.
- **AC-5** Given interactive viewport preview mode, then zero `map_async` readbacks occur on the preview storage buffer and the viewport displays the `PREVIEW` badge (`fid-shlint` clean).

## Design

- **Registry command:** `compare.surface_preview@1`, R1 (Activity Log only), determinism N. Params as `compare.surface_cad@1` plus `max_points?` (subsample cap, default 10⁷) and `mode?: gpu_display | readback_field` (default `gpu_display` in GUI, `readback_field` on CLI/MCP). Result: `PreviewField` (or GPU display handle metadata with `preview: true`) or `preview_unavailable`.
- **Algorithm:** build the nominal BVH on the CPU (GEO.INDEX.BVH), flatten to storage buffers; transform actual points by the alignment on the CPU in f64, then upload camera-relative f32 offsets; compute shader: stack-based BVH traversal, point-triangle closest point, signed distance by face normal, cutoffs; in `gpu_display` mode keep results GPU-resident for direct LUT shading with zero CPU readback (`research/viz-ux-sota/GPU-ARCHITECTURE.md`, D-051); in `readback_field` mode read back into `PreviewField`. Device and queue from VIZ.RENDER.FRAME.
- **Errors:** `preview_unavailable { no_adapter | device_lost | out_of_memory }`, `geo.frame.mismatch`.
- **Determinism:** N (GPU arithmetic is observed).
- **Crate:** `fid-scan-preview` (L3).

## Test fixtures

`panel_scan` (shared with SCAN.CMP.DEV.SURFCAD).

## Build prompt

> In the Rust crate `fid-scan-preview` (layer L3), register `compare.surface_preview@1` (R1, Activity Log only; CLI `fid compare surface-preview --actual <h> --nominal <h> --alignment <h> --method normal|shortest --max-distance L --max-angle A [--max-points N] [--json]`, MCP `compare_surface_preview`). Build the nominal triangle BVH on the CPU with `fid-geo-index` `PrimitiveBvh`, upload it and camera-relative f32 actual points to a WGSL compute shader that returns approximate signed deviations with `NO_MATCH` for distance and angle cutoffs. Support a zero-readback GPU-resident `gpu_display` path for interactive viewport scrubbing (binding the preview storage buffer directly to the display LUT pass with a mandatory `PREVIEW` HUD badge and zero CPU readback, per D-051) and a `readback_field` path returning `PreviewField` (always `preview: true`) for CLI/MCP and `XCHECK`. Get the device from the VIZ.RENDER frame module; with no adapter or on device loss return `preview_unavailable` with a reason. Never write a `DeviationField` or a journal event.
>
> Constraints: CONSTITUTION C2, §3 (R1 not journaled), §8 (no panic/unwrap/unsafe). Tests: AC-1..AC-5.

## Open questions

- Whether a preview is needed in P1b at all is pending CPU timings (parent question); the phase stays P1b until then. Zero-readback GPU-resident scrubbing follows D-051.

## Children

_Leaf: none._
