---
id: VIZ.RENDER.FRAME.GRAPH
title: Frame graph, backend selection and ID and depth buffers
level: 4
parent: VIZ.RENDER.FRAME
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-render]
built_pr: 368
satisfies: [REQ-VIZ.RENDER.FRAME-001, REQ-VIZ.RENDER.FRAME-004]
covers: []
depends_on: [VIZ.RENDER.SCENE.IDTAB, VIZ.RENDER.SCENE.PROJ]
uses_artifacts: [PLAT.GPU.POLICY]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.FRAME.GRAPH: Frame graph, backend selection and ID and depth buffers

## Purpose

The GPU entry point: create the wgpu device on a backend chosen by a documented preference order (overridable by a setting or by an adapter choice injected by the app), run one frame as an ordered graph of passes that other RENDER modules register, and write a colour target, a reversed-Z depth buffer and an ID buffer (render ID plus primitive or point index) for every presented frame. It draws into a wgpu texture and knows nothing about the UI toolkit.

## Scope

**In:** backend and adapter selection; device creation; the pass registration API (`PassId`, declared inputs and outputs, explicit order key); per-frame resource lifetimes; colour, depth and ID targets; presentation into a caller-owned texture.
**Out:** tier classification and `view.get_render_info@1` (`VIZ.RENDER.FRAME.TIER`); shader manifest and lint (`VIZ.RENDER.FRAME.SHLINT`); the passes themselves (`VIZ.RENDER.LOD`, `VIZ.RENDER.SHADE`, `VIZ.RENDER.GLYPH`, `VIZ.OVERLAY`); off-screen targets (`VIZ.RENDER.HEADLESS`); device-loss recovery (`VIZ.RENDER.RECOVER`); the GPU policy files and adapter rules (`PLAT.GPU.POLICY`, `PLAT.GPU.DETECT`).

## Requirements

- **REQ-VIZ.RENDER.FRAME.GRAPH-001** At start-up the frame graph shall try backends in the order given by the backend policy file for the host OS, use the first that yields an adapter, and accept an override from the setting `viz.render.backend` or from an adapter choice injected by the caller.
- **REQ-VIZ.RENDER.FRAME.GRAPH-002** If no backend yields an adapter, then device creation shall fail with `NoAdapter` listing every backend tried and its failure reason.
- **REQ-VIZ.RENDER.FRAME.GRAPH-003** Passes shall be registered with a stable order key and declared inputs and outputs, and the graph shall execute them in ascending order key, rejecting a registration whose input is not produced by an earlier pass with `UnresolvedInput`.
- **REQ-VIZ.RENDER.FRAME.GRAPH-004** Every presented frame shall write a reversed-Z depth buffer and an `Rg32Uint` ID buffer holding render ID and primitive or point index, with 0 meaning no drawable.
- **REQ-VIZ.RENDER.FRAME.GRAPH-005** The frame graph shall draw into a `wgpu::Texture` supplied or exported to the caller and shall expose the ID and depth buffers for CPU readback of a pixel rectangle.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` on the software adapter, when one frame is rendered, then reading the ID buffer at the centre pixel of each visible face returns the face's render ID and a primitive index, and a background pixel returns 0.
- **AC-2** Given a recorded policy file listing `[vulkan, gl]` and a fake instance where Vulkan has no adapter, then the graph selects GL and records the Vulkan failure reason.
- **AC-3** (error path) Given a fake instance with no adapters, then creation fails with `NoAdapter` naming every backend tried.
- **AC-4** (error path) Given a pass that reads an output no earlier pass writes, then registration fails with `UnresolvedInput`.
- **AC-5** Given setting `viz.render.backend = gl`, then GL is used even when the policy prefers another backend.

## Design

- **Registry command(s):** none; library. The backend setting is declared through `CORE.STO.SET` and changed with `settings.set_value@1`.
- **Data structures:** `FrameGraph { device, queue, passes: Vec<PassDecl> }`; `PassDecl { id: PassId, order: u32, inputs: Vec<TargetId>, outputs: Vec<TargetId>, run: Box<dyn Pass> }`; `FrameTargets { color, depth (Depth32Float, reversed-Z), id (Rg32Uint) }`; `AdapterChoice` (caller-injected, e.g. a `PLAT.GPU.DETECT` selection, passed as plain data to avoid a code edge).
- **Algorithm:** read the policy order (artifact `platform/gpu-policy.toml`); try each backend; create device with the limits required by the highest tier the adapter can support; sort passes by order key; validate the input/output chain; per frame clear targets (depth to 0.0 for reversed-Z), execute passes, copy requested rectangles for readback.
- **Errors:** `NoAdapter`, `UnresolvedInput`, `DuplicatePass`, `DeviceLost` (forwarded to `VIZ.RENDER.RECOVER`).
- **Determinism:** N (GPU output); pass order is deterministic.
- **Performance budget:** graph overhead under 0.5 ms per frame excluding passes [unverified].
- **Crate:** `fid-render` (module `frame::graph`).

## Test fixtures

`fixtures/viz/golden-cube` (shared with HEADLESS); a fake wgpu-instance shim with scripted adapter lists; a recorded policy file sample.

## Build prompt

> In crate `fid-render`, add module `frame::graph`. Select a wgpu backend by the policy order read from `platform/gpu-policy.toml` (published by PLAT; read as a file), overridable by setting `viz.render.backend` or an injected `AdapterChoice`; fail `NoAdapter` with every backend's reason. Provide a pass registration API (`PassDecl` with order key and declared inputs/outputs; reject `UnresolvedInput` and `DuplicatePass`), execute passes in order each frame, and maintain colour, reversed-Z `Depth32Float` and `Rg32Uint` ID targets (render ID, primitive/point index; 0 = none) with rectangle readback for picking. Draw into a texture the caller owns; no UI-toolkit dependency. Tests AC-1..AC-5 with `fixtures/viz/golden-cube` and a fake instance. Rules: CONSTITUTION §8; C2 (shaders only map kernel data to pixels); no std HashMap.

## Open questions

- Build: `platform/gpu-policy.toml` is read as an artifact by `fid_render::frame::graph::policy_order` (only `schema_version` and each row's `os`, `arch` and `backends`), not through `fid-platform`, which is a tool-layer crate the viewer layer cannot depend on. Every row is checked: known backend names, each backend once, and `software` last (REQ-PLAT.GPU.POLICY-004). A missing file, no row for the host or an invalid file gives `[software]`, the safe default of `PLAT.GPU.POLICY`, and `FrameGraph::start` keeps the error as `policy_warning()` for the caller to report. The reader accepts `gl` and `webgpu`, which `PLAT.GPU.POLICY`'s schema does not list yet; AC-2's recorded sample (`fixtures/viz/frame-graph/policy-vulkan-gl.toml`) lists `[vulkan, gl, software]`: AC-2's `[vulkan, gl]` followed by the `software` every row must end with; selection stops at GL. `PLAT.GPU.POLICY` should add `gl` (and `webgpu` for the browser build) or AC-2 should name a listed backend.
- Build: precedence is injected `AdapterChoice` first, then the backend the setting `viz.render.backend` names (`auto` or unset keeps the policy), then the policy order, each backend once; a backend that fails is recorded and the next is tried, so an override that yields no adapter falls back to the policy with its reason in `FrameGraph::tried`. A setting that names no backend is `bad_setting`. `software` means a CPU adapter (`DeviceType::Cpu`) on any compiled backend; without a choice the hardware backends skip CPU adapters, while an injected choice on a hardware backend is honoured whatever the adapter type (a WARP choice on dx12 included); `software` stays CPU-only with or without a choice (`pick_adapter`).
- Build: wgpu 27 is built with the `dx12` and `vulkan` backends only (as in the `VIZ.SHELL.SPKTREE` spike: `metal` pulls `paste`, RUSTSEC-2024-0436, and `gles` is not vetted against deny.toml). So macOS has no backend in this build and GL is not compiled anywhere: requests for them fail with "backend not compiled into this build". REQ-FRAME-001 ("every backend the pinned wgpu exposes") needs `metal` and `gles` once their dependencies pass deny.toml.
- Build: AC-1 runs on the software adapter wherever one exists. On GitHub's Windows runners that is WARP (DX12), where the test must pass; the Linux and macOS runners have no CPU adapter (no lavapipe or llvmpipe installed), so the test returns early there, but only on `NoAdapter` whose every reason says no adapter exists or nothing is compiled in; any other error fails it. AC-2..AC-5 use a scripted `AdapterSource` fake and run everywhere. The golden cube's vertices are given in clip space by `fixtures/viz/golden-cube/make_fixtures.py`, which also labels the expected IDs, primitive pairs and reversed-Z depths (the face centre lies on the shared diagonal, so either of the face's two primitives is accepted).
- Build: a pass's inputs are checked when it registers: each must be a frame target (`colour`, `depth`, `id`, cleared before the first pass) or an output of an already registered pass with a lower order key, so producers register before consumers. Two passes with the same order key are refused (`DuplicateOrder`, not in the error list above) so the order never depends on registration order. Pass-owned transient targets are logical names only: per-frame allocation of transient textures (the Scope's "per-frame resource lifetimes") is left to the passes until a pass needs a shared transient.
- Build (Windows CI on WARP, review of #368): WebGPU copies depth textures only whole, so `read_depth` copies the whole depth target to a staging buffer and crops the rectangle on the CPU (`crop_rows`); a target whose staging buffer exceeds the device's `max_buffer_size` is `readback_too_large`. ID readback (`Rg32Uint`) still copies only the rectangle. Possible follow-up for high-resolution picking: a pass that copies depth into an `R32Float` target so a rectangle can be copied alone.
- Build: the device is created with no optional feature and the adapter's own limits; narrowing to a tier's limits is `VIZ.RENDER.FRAME.TIER`. Target creation, frames and readbacks run inside validation and out-of-memory error scopes (`gpu`), and an error outside any scope is kept by an `on_uncaptured_error` handler instead of wgpu's panicking default and returned as `gpu` by the next call; a lost device is `device_lost` on the next call.
- Cross-domain: `PLAT.GPU.DETECT` declares `depends_on: [VIZ]`, so this leaf cannot take a code edge to it without a cycle. It reads the policy file as an artifact and accepts the selection as injected data; PLAT.GPU.DETECT should narrow its dependency to this node or drop it.

## Children

_Leaf: none._
