---
id: D-051
title: GPU-driven visibility buffer, linear HDR pipeline and optional T0_RT tier
status: open
raised_by: founder
blocks: [VIZ.RENDER.SHADE.RT]
decide_by: before P1b mesh/CAD/preview leaves are claimed
updated: 2026-11-22
---

# D-051: GPU-driven visibility buffer, linear HDR pipeline and optional T0_RT tier

## Question

Should `VIZ.RENDER` and `SCAN.CMP.GPUPV.PVGPU` adopt a **GPU-driven Visibility Buffer architecture**, linear `Rgba16Float` HDR + OKLab Eye-Dome Lighting, compressed chunk-local point/surfel layouts, zero-readback interactive GPU deviation previews, an `objc2-metal` wgpu backend upgrade, and an optional **`T0_RT` hardware ray-query workstation tier** (display-only, strictly under CONSTITUTION C2)?

## Context

`VIZ.RENDER` already writes an `Rg32Uint` ID buffer (`render_id`, `primitive_index`) and reversed-Z `Depth32Float` target every frame (`VIZ.RENDER.FRAME.GRAPH`), while shading points, meshes, and CAD in separate forward passes driven by CPU culling loops. Meanwhile:

1. **Under-utilised GPU pipeline**: CPU frustum culling and per-digest instanced draw loops (`VIZ.RENDER.SHADE.CAD`), non-LOD mesh rendering (`VIZ.RENDER.SHADE.MESH`), and full-depth CPU readbacks (`VIZ.RENDER.FRAME.GRAPH` Open questions) create CPU submission and readback bottlenecks at 10⁸ points, 10⁷+ triangles, and 5k+ part CAD assemblies [unverified].
2. **Feature starvation and missing macOS Metal backend**: `crates/fid-render/Cargo.toml` omits `metal` (because the older `metal` crate pulled `paste`, RUSTSEC-2024-0436) and requests zero optional `wgpu::Features`, leaving `SHADER_INT64_ATOMIC_MIN_MAX`, `SUBGROUP`, `MULTI_DRAW_INDIRECT`, and `TIMESTAMP_QUERY` unused, and blocking Apple Silicon unified-memory workstations.
3. **Color-space and hue accuracy**: `COLOUR_FORMAT = Rgba8Unorm` lacks linear HDR headroom; multiplying RGB for Eye-Dome Lighting can shift LUT hues near `VIZ.RENDER.LOD.ATTR` AC-3's `< 1°` budget.
4. **Unnecessary readback in `SCAN.CMP.GPUPV.PVGPU`**: Interactive scrubbing currently specifies reading back up to 10⁷ GPU preview floats to CPU before re-uploading for display, adding latency and widening the readback surface that `fid-shlint` audits.
5. **Workstation ray-query opportunity**: Modern Vulkan, DX12, and Metal GPUs expose hardware ray queries (BLAS/TLAS) through `wgpu`, enabling display-only contact AO, live laser-tracker line-of-sight shadow volumes (`Network`), and probe/scanner occlusion previews (`Probe`/`Inspect`) without ever producing a value of record.

See `research/viz-ux-sota/GPU-ARCHITECTURE.md` for full pass layouts, memory budgets, WGSL contracts, and `fid-shlint` proof obligations.

## Options

| Option | For | Against |
|---|---|---|
| A. Keep classic CPU-driven forward pipeline (`Rgba8Unorm`, no meshlets, no indirect culling, no RT tier) | Minimal spec churn; matches initial P1a scaffold | Leaves 5–10× GPU throughput unused on large assemblies and 10⁸-point scans [unverified]; stalls on hover depth readback; omits Apple Silicon until `metal` is replaced |
| B. **GPU-driven Visibility Buffer + Linear HDR/OKLab + Zero-Readback Preview + Optional `T0_RT` tier** (Recommended) | Unifies point/mesh/CAD shading with zero overdraw shading waste and zero-cost ID buffers; cuts point VRAM 40–60% via quantized offsets + octahedral normals; eliminates interactive `PVGPU` readbacks (stronger C2 enforcement); unlocks Apple Silicon via `objc2-metal`; enables real-time AO and line-of-sight previews on T0+ GPUs while degrading cleanly to T0/T1/T2 | Adds meshlet preprocessing on import/tessellation (`GEO`/`VIZ.RENDER`) and requires `wgpu` version bump for `objc2-metal` and ray queries (vetted against `deny.toml` and MSRV) |

## Recommendation

Accept **Option B**, phased cleanly so P1a is never blocked:

1. **P1a renderer upgrades** (non-blocking follow-up after PR #450 `VIZ.RENDER.FRAME.TIER` lands):
   - Upgrade `VIZ.RENDER.FRAME.GRAPH` to linear `Rgba16Float` colour target with calibrated sRGB/Display-P3 resolve, async ring-buffered tile `R32Float` depth readback, and opt-in tier features (`SHADER_INT64_ATOMIC_MIN_MAX`, `SUBGROUP`, `MULTI_DRAW_INDIRECT`, `TIMESTAMP_QUERY`); upgrade `wgpu` to an `objc2-metal` release once it passes `deny.toml` and workspace MSRV.
   - Adopt the **Unified Visibility Buffer** across `VIZ.RENDER.LOD.RASTER` (subgroup-reduced `compute64`, optional HQS/surfels, quantized offsets + 16-bit octahedral normals), `VIZ.RENDER.LOD.ATTR` (OKLab lightness EDL, uncertainty hatch, bin-hover isolation), and `VIZ.RENDER.SHADE.MESH` (meshlet cluster LOD, SDF edges, MatCap/Zebra/Curvature display modes, display-only deviation exaggeration).
   - Add T0 K-buffer / blue-noise stochastic ghost transparency alongside WBOIT in `VIZ.RENDER.SHADE.OIT`.
2. **P1b scale upgrades**:
   - GPU-driven Hi-Z + `multi_draw_indexed_indirect_count` assembly culling in `VIZ.RENDER.SHADE.CAD`.
   - Zero-readback GPU-resident interactive preview mode in `SCAN.CMP.GPUPV.PVGPU` (readback only when CLI/MCP explicitly requests a `PreviewField`).
3. **P2 workstation tier**:
   - Add optional `T0_RT` hardware ray-query display pass in `VIZ.RENDER.SHADE.RT` (contact AO, laser-tracker line-of-sight volumes, probe/scanner occlusion previews), strictly C2-compliant and audited by `fid-shlint` with zero readbacks.

## Decision

_Open._ Requires maintainer approval (D-037 P1). All rendering outputs remain strictly display-only (CONSTITUTION C2) and verified against `VIZ.RENDER.ORACLE` and `fid-shlint`.
