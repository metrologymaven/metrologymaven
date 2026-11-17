---
id: VIZ.RENDER.HEADLESS
title: Headless off-screen renderer
level: 3
parent: VIZ.RENDER
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-010]
covers: []
depends_on: [VIZ.RENDER.FRAME]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.RENDER.HEADLESS: Headless off-screen renderer

## Purpose

Render any scene and view state into an off-screen image with no window: for report images, CI golden images, server-rendered stills (tier T3) and the capture commands. On machines without a usable GPU it falls back to the software adapter. Every image comes with a manifest binding it to the digests of what it depicts, because pixels are illustrations, not records.

## Scope

**In:** off-screen render targets, software-adapter fallback, readback to CPU memory, the image manifest.
**Out:** screenshot and animation commands (`VIZ.CAPTURE`); view state (`VIZ.VIEW`); report placement (`RPT`); CPU reference rasterizer (`VIZ.RENDER.ORACLE`).

## Requirements

- **REQ-VIZ.RENDER.HEADLESS-001** The renderer shall render a scene and view state into an off-screen RGBA8 sRGB image at any requested width and height from 16 to 8192 pixels without creating a window or surface.
- **REQ-VIZ.RENDER.HEADLESS-002** When no hardware adapter is available, the headless renderer shall use the software fallback adapter and shall report that adapter in the image manifest.
- **REQ-VIZ.RENDER.HEADLESS-003** Every headless image shall be returned with a manifest holding the scene digest, view-state digest, capability tier, adapter name, size and image SHA-256.
- **REQ-VIZ.RENDER.HEADLESS-004** If the requested size exceeds the adapter's maximum texture dimension, then the renderer shall fail with error `SizeExceedsAdapterLimit` and return no image.
- **REQ-VIZ.RENDER.HEADLESS-005** Rendering the same scene and view twice on the same adapter shall produce byte-identical images.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` and its stored view, when `render_offscreen` runs at 640×480 on the software adapter, then the image matches `golden-cube.png` within the per-pixel tolerance mask stored with the golden.
- **AC-2** Given the same inputs rendered twice in one process, then both images have the same SHA-256.
- **AC-3** Given a host where hardware adapters are disabled by test flag, when rendering runs, then it succeeds and the manifest names the software adapter.
- **AC-4** (error path) Given a requested width of 65 536, when rendering runs, then it fails with `SizeExceedsAdapterLimit` and no image is returned.
- **AC-5** Given any successful render, then the manifest's scene and view digests equal the digests computed independently from the inputs.

## Design

- **Registry command(s):** none; library API used by `VIZ.CAPTURE`, `VIZ.VIEW` restore tests, `RPT` and CI.
- **API:** `fid_render::headless::render_offscreen(scene: &SceneGraph, view: &ViewStateRef, size: Extent2d, opts: HeadlessOptions) -> Result<OffscreenImage, HeadlessError>`; `OffscreenImage { rgba: Vec<u8>, manifest: ImageManifest }`.
- **Algorithm:** request an adapter with no compatible surface; fall back to `force_fallback_adapter`; build the same frame graph as on screen targeting an off-screen texture; copy to a mapped buffer with row-padding removal; hash the unpadded rows.
- **Errors:** `NoAdapter`, `SizeExceedsAdapterLimit`, `DeviceLost` (forwarded to recovery), `Readback`.
- **Determinism:** N across adapters (pixels are not bit-reproducible across GPUs); byte-identical on the same adapter and driver (REQ-005).
- **Performance budget:** 1920×1080 still of the benchmark scene under 500 ms on the software adapter [unverified].
- **Crate:** `fid-render` (module `headless`).

## Test fixtures

`fixtures/viz/golden-cube`: a unit cube with three coloured faces and a stored view, generated in-repo (no external licence); golden image and tolerance mask regenerated only by an explicit maintainer command.

## Build prompt

> In crate `fid-render`, add module `headless` with `render_offscreen(scene, view, size, opts) -> Result<OffscreenImage, HeadlessError>`. Create the device without a surface; if no hardware adapter exists use wgpu's fallback (software) adapter. Render the normal frame graph into an RGBA8 sRGB off-screen texture of 16–8192 px per side; reject sizes above the adapter limit with `SizeExceedsAdapterLimit`. Read back, strip row padding, compute SHA-256, and return an `ImageManifest { scene_digest, view_digest, tier, adapter, width, height, image_sha256 }`. Tests: golden image `golden-cube` at 640×480 within its tolerance mask; two renders give identical hashes; forced software adapter works and is named in the manifest; oversize request fails; manifest digests match inputs. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; no UI-toolkit dependency; images are illustrations and carry digests, never values of record.

## Open questions

- Tiled rendering for sizes above adapter limits may be added later if reports need it.

## Children

_Leaf: none._
