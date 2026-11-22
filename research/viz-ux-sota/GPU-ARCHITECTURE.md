# State-of-the-Art GPU Rendering Architecture (`VIZ.RENDER` & `SCAN.CMP.GPUPV`)

Companion technical design for **D-051** (`decisions/D-051-gpu-driven-visibility-buffer-and-rt.md`). Every technique below obeys **CONSTITUTION C1/C2** (the GPU never computes a value of record, colour bin, pass/fail verdict, or selection identity; `fid-shlint` audits every WGSL module and `map_async` call site).

## 1. Unified Visibility Buffer Frame Graph

`VIZ.RENDER.FRAME.GRAPH` already mandates writing a reversed-Z `Depth32Float` buffer and an `Rg32Uint` ID buffer (`render_id`, `primitive_or_point_index`) every frame. Instead of forward-shading every point, mesh triangle, and CAD face during geometry rasterisation, unify all opaque passes into a **Visibility Buffer**:

```text
Pass 10  hiz_pyramid_prev      Compute: build Hi-Z depth pyramid from previous frame (temporal occlusion)
Pass 20  gpu_cull_and_lod      Compute: frustum + backface-cone + Hi-Z + sub-pixel cull for
                                        CAD instances, meshlets, and resident point chunks
                                        -> compacted DrawIndexedIndirectArgs + compute-raster queues
Pass 30  vis_points_t0         Compute: subgroup-reduced u64 atomicMax(rev_z | packed_vis_id)
                                        (T1: two-pass u32; T2: instanced sprites -> depth + ID)
Pass 35  vis_meshlets_sw       Compute: u64 atomicMax software raster for sub-pixel meshlets (T0)
Pass 40  vis_surfaces_hw       Render:  multi_draw_indexed_indirect_count for large meshlets & CAD
                                        writing ONLY reversed-Z depth + Rg32Uint ID buffer
Pass 45  vis_clip_caps         Render:  stencil-capped display clip planes writing ClipCap derived ID
Pass 50  vis_shade_deferred    Compute: single full-screen pass per visible pixel:
                                        fetch material/LUT bin/normal/attr from storage buffers,
                                        evaluate PBR/MatCap/Zebra/Curvature, apply uncertainty hatch,
                                        hover-to-isolate dimming, and stale desaturation -> Rgba16Float
Pass 55  edl_oklab             Compute: Eye-Dome Lighting modifying OKLab lightness L* only
Pass 60  rt_contact_ao (T0_RT) Compute: optional hardware ray-query contact AO & LoS shadow overlay
Pass 70  oit_transparent       Render/Compute: T0 4-8 layer K-buffer or stochastic ghost pass
                                               (T1/T2: WBOIT; headless report: exact depth peeling)
Pass 80  edges_and_overlays    Render:  analytical SDF anti-aliased CAD/feature edges, MTSDF GD&T
                                        FCFs, 3D tolerance-zone ghosts, leaders, wipe curtain
Pass 90  tonemap_resolve       Compute/Render: Rgba16Float linear -> calibrated sRGB / Display P3
Pass 95  async_pick_copy       Compute: copy 1x1 or pick-tile Depth32Float -> R32Float + ID ring buf
```

### Why this matters for Metrology Maven
- **Zero overdraw shading waste**: Complex CAD assemblies and dense overlapping point clouds shade each screen pixel once.
- **ID buffer for free**: `VIZ.PICK.RESOLVE` reads the exact same `Rg32Uint` visibility buffer—no duplicate ID pass or divergence between shaded pixels and picked primitives.
- **Strict C2 isolation**: Geometry passes only write visibility IDs; LUT indexing in `vis_shade_deferred` reads integer kernel bin indices verbatim without arithmetic on deviation values.

## 2. Point Cloud Compression, Subgroups & High-Quality Splatting (`VIZ.RENDER.LOD`)

### Compressed Chunk-Local Point Layout (10–12 bytes/point vs 28–36 bytes/point)
Every chunk has an f64 origin on the CPU and half-extent ≤ 1 m (`VIZ.RENDER.PREC`):
- **Position**: `[u16; 3]` chunk-local quantised offset (6 bytes; ~30.5 µm quantisation across a 2 m chunk, or split to ≤ 64 mm leaf chunks when zoomed close for ~1 µm display fidelity; CPU f64 coordinates remain authoritative for picking and kernel math).
- **Normal**: `u32` (or `[i16; 2]`) **16-bit octahedral-encoded unit normal** (2 bytes) for shaded surfels and normal-weighted EDL.
- **Attribute / Kernel Bin**: `u16` packed `(kernel_bin_index: 8 bits, class_flags: 4 bits [excluded, indeterminate, selected, stale], pass_id: 4 bits)` (2 bytes), with optional auxiliary attribute stream (`RGB8`, `intensity`, `timestamp`, `uncertainty`) bound only when that `AttrMode` is active.
- **Impact**: Cuts GPU VRAM footprint by ~55–65%, letting the 70% LRU chunk cache (`VIZ.RENDER.LOD.CACHE`) hold **2–3× more resident points** before eviction [unverified].

### WGSL Subgroup Reduction + HQS Surfels (`VIZ.RENDER.LOD.RASTER`)
- **Subgroup dedup (`enable subgroups;`)**: Before issuing `atomicMax` on the 64-bit `(reversed_z_u32 << 32) | vis_id` visibility buffer, lanes within a subgroup targeting the same screen pixel reduce locally with `subgroupMin`/`subgroupBroadcast`, cutting VRAM atomic contention by 2–4× on dense zoomed-out scans [unverified].
- **High-Quality Splatting (HQS) & Oriented Surfels**: When zoomed in past 1 point per pixel, an optional display toggle switches from 1 px splats to **oriented circular surfels** (radius derived from chunk LOD spacing, tilted by the octahedral normal) with a two-pass depth + epsilon-band weighted blend so zoomed scans read as clean continuous surfaces without holes or sparkle.

## 3. GPU-Driven Assembly Culling & Meshlet Cluster LOD (`VIZ.RENDER.SHADE`)

- **Meshlets (`VIZ.RENDER.SHADE.MESH`)**: Meshes exceeding 64k triangles are partitioned into 64–128 triangle **meshlets** organised into a hierarchical cluster DAG (built deterministically on CPU when uploading to GPU cache). Each meshlet stores a chunk-local bounding sphere, normal cone (for backface culling), and screen-space geometric error.
- **GPU Culling (`VIZ.RENDER.SHADE.CAD` & `MESH`)**: A single WGSL compute pass tests every CAD instance and meshlet against the 6 frustum planes, normal cone, previous-frame Hi-Z depth pyramid, and pixel-size threshold (default 2 px), writing surviving commands into indirect draw buffers executed via `multi_draw_indexed_indirect_count`.
- **Inspection Display Shaders (C2-safe)**:
  - **Analytical SDF Anti-Aliased Edges**: Feature edges and B-rep polylines render as camera-facing quad strips with fragment signed-distance coverage (`fwidth`-normalised), eliminating jagged 1 px aliased lines on 4K/HiDPI displays.
  - **Zebra / Isophote Reflection Lines, Studio MatCap & Curvature Preview**: Evaluated inside `vis_shade_deferred` from screen-space / vertex normals strictly as visual inspection aids (never producing stored numbers).
  - **Display-Only Deviation Exaggeration**: Optional vertex displacement `pos + normal * (kernel_dev_f32 * exaggeration_factor)` (`1×` to `500×`) for visualising micrometre-scale warpage on large panels; explicitly disabled when picking or flagged as visually exaggerated in the viewport HUD.

## 4. Linear HDR, OKLab Eye-Dome Lighting & Modern OIT

- **Linear `Rgba16Float` Target**: All shading and post-processing run in linear `Rgba16Float`, resolving to calibrated sRGB or Display-P3 at presentation. This eliminates 8-bit gradient banding on subtle continuous deviation maps.
- **OKLab Lightness EDL (`VIZ.RENDER.LOD.ATTR`)**: Convert LUT RGB -> OKLab `(L, a, b)`, modulate **only** `L` by the Eye-Dome Lighting shade factor, and convert back to linear RGB (gamut-clamped at constant hue angle `atan2(b, a)`). This mathematically guarantees `< 1°` hue shift (REQ-VIZ.RENDER.LOD.ATTR-004 / AC-3).
- **Interactive Transparency (`VIZ.RENDER.SHADE.OIT`)**:
  - **T0**: Fixed-depth **K-buffer / per-pixel linked list** (4–8 layers in a bounded storage buffer) for crisp, colour-accurate CAD-over-scan transparency, plus **blue-noise stochastic alpha** for ultra-cheap proposal ghost overlays.
  - **T1/T2**: Weighted Blended OIT fallback.
  - **Headless / Report Quality**: Exact N-layer depth peeling (default 8 layers) matched against `VIZ.RENDER.ORACLE`.

## 5. Async Tile Depth Readback, Zero-Readback Preview & `objc2-metal`

- **Async Ring-Buffered Pick Readback (`VIZ.RENDER.FRAME.GRAPH` follow-up)**: Replace synchronous whole-texture `read_depth` copies with a tiny compute pass that copies only the requested pixel rectangle from `Depth32Float` into an `R32Float` staging buffer inside a 3-slot ring buffer, eliminating frame stalls during pointer hover (`VIZ.PICK.READOUT`).
- **Zero-Readback GPU Preview (`SCAN.CMP.GPUPV.PVGPU`)**: During interactive slider scrubbing (alignment, search distance, angle cutoff, colour-scale handles), `compare.surface_preview@1` writes approximate deviations into a **GPU-resident storage buffer** bound directly by `vis_shade_deferred` with an unmistakable viewport `PREVIEW — NOT VALUE OF RECORD` banner and **zero CPU readback**. `PreviewField` CPU readback runs only when a CLI/MCP caller explicitly requests the structured preview field.
- **`objc2-metal` & Tier Features**: Upgrade `wgpu` to a release backed by `objc2-metal` (removing the unmaintained `paste` transitive dependency from the legacy `metal` crate) once vetted against `deny.toml` and workspace MSRV, enabling native Apple Silicon Metal builds. Enable tier-gated `wgpu::Features`: `SHADER_INT64_ATOMIC_MIN_MAX`, `SUBGROUP`, `MULTI_DRAW_INDIRECT`, `MULTI_DRAW_INDIRECT_COUNT`, `TIMESTAMP_QUERY`, and (on T0_RT) `EXPERIMENTAL_RAY_QUERY` / ray-tracing extensions as stabilised in `wgpu`.
- **Dirty-Driven Viewport Caching**: When camera, scene digest, and streaming residency are unchanged, cache the resolved viewport texture and composite UI/cursor overlays over it at native display refresh rate (`ControlFlow::Wait`), eliminating idle GPU burn.

## 6. Optional `T0_RT` Hardware Ray-Query Tier (`VIZ.RENDER.SHADE.RT`, P2)

On Vulkan/DX12/Metal GPUs supporting hardware ray queries (BLAS/TLAS):
- **Contact Ambient Occlusion**: 2–4 short-range ray queries per visible pixel in `vis_shade_deferred` (temporally filtered) to clarify bore depth, step edges, and fixture clearances.
- **Live Line-of-Sight & Occlusion Volumes**: For *Network* (laser tracker / radar station placement) and *Probe*/*Inspect* (scanner/probe reachability), cast display-only visibility rays from instrument origins against scene BLAS/TLAS to shade occluded regions in real time.
- **Strict C2 & `fid-shlint` Contract**: `T0_RT` passes have **zero CPU readbacks** (declared colour-only output in `shaders/manifest.toml`), never influence `ResultRecord`s or collision-check tickets (`CMM.COLL` stays pure CPU f64), and degrade silently to non-RT shading on T0/T1/T2 hardware.
