---
id: VIZ.RENDER.SHADE.RT
title: Optional T0_RT hardware ray-query display overlays
level: 4
parent: VIZ.RENDER.SHADE
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.SHADE-001]
covers: []
depends_on: [VIZ.RENDER.SHADE.MESH, VIZ.RENDER.FRAME.TIER]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# VIZ.RENDER.SHADE.RT: Optional T0_RT hardware ray-query display overlays

## Purpose

An optional workstation enhancement (capability flag `T0_RT`, governed by **D-051**) that uses hardware ray queries (`wgpu` BLAS/TLAS on Vulkan, DX12 and Metal GPUs that expose ray-tracing extensions) strictly for **display-only visual aids**: short-range contact ambient occlusion to clarify bores, pockets and fixture clearances, live laser-tracker/radar line-of-sight shadow volumes in *Network*, and probe/scanner occlusion previews in *Probe* and *Inspect*. It produces zero values of record, issues zero CPU readbacks, and degrades cleanly to non-RT shading on T0/T1/T2 adapters.

## Scope

**In:** BLAS/TLAS acceleration structure builder synchronized from `VIZ.RENDER.SCENE` geometry chunks; WGSL ray-query compute pass for temporally filtered contact AO (2–4 rays/pixel over the Visibility Buffer); live instrument line-of-sight shadow overlay from kernel-supplied station/probe f64 origins; graceful no-op fallback when `T0_RT` is unavailable; shader manifest entries with zero readback targets.
**Out:** authoritative collision checking (`CMM.COLL`, pure CPU f64), authoritative deviation computation (`SCAN.CMP.DEV`, pure CPU f64), station uncertainty propagation (`LVM`/`UNC`), ray-traced values of record (forbidden by CONSTITUTION C2).

## Requirements

- **REQ-VIZ.RENDER.SHADE.RT-001** When the adapter exposes hardware ray-query support (`T0_RT`), the renderer shall build display-only BLAS/TLAS acceleration structures from scene geometry chunks and offer toggleable contact ambient occlusion and live instrument line-of-sight shadow overlays.
- **REQ-VIZ.RENDER.SHADE.RT-002** When the adapter lacks hardware ray-query support, the pass shall deactivate cleanly with no error and leave T0/T1/T2 rendering unchanged.
- **REQ-VIZ.RENDER.SHADE.RT-003** Every `T0_RT` shader shall write only to the linear HDR colour target (never to an ID buffer or CPU-mapped buffer) and shall pass `fid-shlint` with zero readback targets.
- **REQ-VIZ.RENDER.SHADE.RT-004** Contact AO shall modulate only display lightness in OKLab space, preserving LUT hue within 1 degree on deviation-mapped surfaces.
- **REQ-VIZ.RENDER.SHADE.RT-005** The `T0_RT` contact AO and line-of-sight pass shall execute in under 3 ms at 1920 × 1080 on a `T0_RT`-capable reference GPU [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` with an occluding plate and a station origin on a `T0_RT` adapter, when the line-of-sight overlay is enabled, then occluded pixels render with the line-of-sight shadow tint and unoccluded pixels do not, while the ID buffer is byte-identical to the non-RT pass.
- **AC-2** Given an adapter without ray-query support (including the software adapter), when the scene renders with RT overlays requested, then the pass degrades to a no-op with a `RtUnavailable` diagnostic and zero errors.
- **AC-3** Given `fid-shlint` run on `fid-render`, then all `T0_RT` WGSL modules are listed in `shaders/manifest.toml` with colour-only output and zero readback call sites.
- **AC-4** Given a deviation-mapped mesh with `T0_RT` contact AO enabled, then sampled HSV hues of LUT colours differ from AO-off by less than 1 degree.

## Design

- **Registry command(s):** none; controlled by viewport display-style toggles (`viz.render.rt_ao`, `viz.render.rt_los`).
- **Data structures:** `RtSceneBlas { chunks: BTreeMap<ChunkId, BlasHandle>, tlas: TlasHandle }`; `LosEmitter { origin_rte: [f32; 3], max_range_m: f32, kind }`.
- **Algorithm:** build/refit BLAS per resident chunk from chunk-local f32 vertices; build TLAS with relative-to-eye chunk transforms; in compute pass over the Visibility Buffer, trace 2–4 cosine-weighted short rays for contact AO (attenuating OKLab `L*` only) and 1 shadow ray per active `LosEmitter` for line-of-sight shading; apply lightweight temporal exponential filter reset on camera motion or scene digest change.
- **Errors:** `RtUnavailable` (informational diagnostic; falls back silently), `DeviceLost` (forwarded to `VIZ.RENDER.RECOVER`).
- **Determinism:** N (display-only GPU visual enhancement).
- **Performance budget:** < 3 ms at 1920 × 1080 on a `T0_RT` workstation GPU [unverified].
- **Crate:** `fid-render` (module `shade::rt`).

## Test fixtures

`fixtures/viz/golden-cube` with an added occluder plate and emitter origin; `fid-shlint` manifest check.

## Build prompt

> In crate `fid-render`, add module `shade::rt` implementing optional `T0_RT` hardware ray-query display overlays (contact ambient occlusion and live instrument line-of-sight shadow volumes) per `research/viz-ux-sota/GPU-ARCHITECTURE.md` and D-051. Build chunk-local BLAS and relative-to-eye TLAS from scene chunks when the adapter exposes hardware ray queries; run a compute pass over the Visibility Buffer casting 2–4 short AO rays (modulating OKLab lightness `L*` only so LUT hues stay within 1°) and 1 visibility ray per active station/probe origin; degrade cleanly to a no-op with `RtUnavailable` diagnostic on non-RT adapters. Register all WGSL modules in `shaders/manifest.toml` with colour-only output and zero CPU readbacks (`fid-shlint` clean). Tests AC-1..AC-4. Rules: CONSTITUTION §8, C2 (never compute values of record or collision verdicts on GPU); no std HashMap.

## Open questions

- Governed by open decision **D-051**; depends on `wgpu` ray-query API stabilisation in the workspace-pinned `wgpu` version.

## Children

_Leaf: none._
