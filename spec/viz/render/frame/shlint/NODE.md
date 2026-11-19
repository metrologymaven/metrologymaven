---
id: VIZ.RENDER.FRAME.SHLINT
title: Shader manifest, readback lint and toolkit-free check
level: 4
parent: VIZ.RENDER.FRAME
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-shlint]
built_pr: 207
satisfies: [REQ-VIZ.RENDER.FRAME-005, REQ-VIZ.RENDER.FRAME-006]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# VIZ.RENDER.FRAME.SHLINT: Shader manifest, readback lint and toolkit-free check

## Purpose

Enforce C2 mechanically: every shader module is listed in a manifest with its declared inputs and outputs, CI fails if non-test code reads back any GPU output other than colour, depth or ID targets, and CI fails if the render crate gains a dependency on a UI-toolkit crate. These are build-time checks, so they live in a small tooling binary.

## Scope

**In:** `shaders/manifest.toml` schema; a lint that parses WGSL entry points and `map_async` call sites; a dependency-graph check of `fid-render`.
**Out:** golden-image comparison (`VIZ.RENDER.ORACLE.GATE`); the frame graph (`VIZ.RENDER.FRAME.GRAPH`).

## Requirements

- **REQ-VIZ.RENDER.FRAME.SHLINT-001** Every WGSL module in `fid-render` shall appear in `shaders/manifest.toml` with its bindings classified as input data, colour target, depth target or ID target, and the lint shall fail on any unlisted module or binding.
- **REQ-VIZ.RENDER.FRAME.SHLINT-002** The lint shall fail if any non-test source maps a GPU buffer or texture for reading other than one declared as a colour, depth or ID target.
- **REQ-VIZ.RENDER.FRAME.SHLINT-003** The lint shall fail if `cargo metadata` shows `fid-render` depending, directly or transitively, on a crate in the UI-toolkit denylist (`egui`, `eframe`, `winit`, `iced`, `slint` and their sub-crates), unless that crate is listed under `[[allow]]` in `shaders/manifest.toml` with a reason, in which case the lint shall report it as an informational `SHLINT-003` finding carrying the reason and shall not fail the run.
- **REQ-VIZ.RENDER.FRAME.SHLINT-004** Each lint failure shall name the file, line or crate path and the rule broken.

## Acceptance criteria

- **AC-1** Given fixture crate `fixtures/viz/shlint/ok`, when the lint runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/viz/shlint/unlisted` (a WGSL module missing from the manifest), then it exits 1 naming the module.
- **AC-3** (error path) Given `fixtures/viz/shlint/readback` (a storage buffer mapped for read in non-test code), then it exits 1 naming the file and line.
- **AC-4** (error path) Given `fixtures/viz/shlint/toolkit` (a transitive `winit` dependency), then it exits 1 naming the path of crates.

## Design

- **Registry command(s):** none; CI tool `fid-shlint` (tooling layer, reads source and `cargo metadata` output only).
- **Data structures:** `ShaderManifest { modules: Vec<{ path, bindings: Vec<{ group, binding, name, class }> }>, targets: Vec<{ name, class }>, allows: Vec<{ crate, reason }> }`. In `shaders/manifest.toml` these are `schema = 1`, `[[module]]` with `[[module.binding]]`, `[[target]]` and `[[allow]]`. A `[[target]]` declares a readback target that is not a shader binding, such as the copy buffer of a colour, depth or ID attachment; its class is `colour`, `depth` or `id`, never `input`. An `[[allow]]` names one crate from the UI-toolkit denylist with a one-line reason. The lint still reports that crate, as an informational finding (`SHLINT-003/toolkit-allowed`) carrying the reason, never silently, and the finding does not fail the run.
- **Algorithm:** parse WGSL with `naga` front end for bindings; scan Rust sources for `map_async`/`slice(..).map_async` with a target name matched against the manifest; walk the dependency graph from `cargo metadata --format-version 1`.
  - Receivers are matched by their **last identifier only**: `self.frame.id_readback.slice(..)` matches the name `id_readback`. A name that any `input` binding uses is denied. A receiver that is not a plain name, or a call through `self` or a path, is an `unresolved-readback` finding.
  - wgpu readback helpers that bypass `map_async` attribution (`wgpu::util::DownloadBuffer`, `read_buffer`) are always `unresolved-readback` findings.
  - Shader text that bypasses the manifest is an `unmanifested-shader` finding. This covers inline strings in `ShaderSource::Wgsl(..)`, a `ShaderSource::Wgsl(..)` without an include macro, other `ShaderSource` variants, `include_spirv!`, and `include_str!`/`include_bytes!`/`include_wgsl!` of anything but a listed `.wgsl` module (the path is resolved relative to the source file).
  - The metadata must describe the crate being linted: the package's `manifest_path` must lie in `--crate-dir`, otherwise the run exits 2. CI must produce the metadata in the same job with `cargo metadata --format-version 1 --all-features --locked`, without `--filter-platform`, so that every feature and target platform is in the graph.
- **Errors:** findings as above; `ManifestMalformed`; `MetadataMalformed`; `MetadataMismatch` (exit 2). Exit codes: 0 when there are no findings other than informational ones, 1 when there are findings, and 2 when an input cannot be read or parsed.
- **Determinism:** D0.
- **Performance budget:** under 10 s on CI.
- **Crate:** `fid-shlint` (tool).

## Test fixtures

`fixtures/viz/shlint/{ok,unlisted,readback,toolkit}`: tiny project-authored crates.

## Build prompt

> Create CI tool crate `fid-shlint`. It reads `fid-render/shaders/manifest.toml` (each WGSL module and binding classified input/colour/depth/id), parses WGSL with naga to check every module and binding is listed, scans non-test Rust sources for GPU readbacks of anything other than colour, depth or ID targets, and checks with `cargo metadata` that `fid-render` has no direct or transitive dependency on egui, eframe, winit, iced or slint crates. Each finding names file/line or crate path and rule; exit 1 on any finding. Tests AC-1..AC-4 with fixtures under `fixtures/viz/shlint/`. Rules: CONSTITUTION §8; C2.

## Open questions

- No CI step runs `fid-shlint` on a real crate until `fid-render` exists. The CI wiring (metadata command above, then `fid-shlint --crate-dir crates/fid-render --metadata <file>`) lands with the first `fid-render` PR, and until then only the fixtures exercise the tool.
- The toolkit match is by name prefix (`<toolkit>-`/`<toolkit>_`, plus `epaint`, `emath`, `ecolor` and `i-slint-*`), so unrelated crates that share a prefix match too, for example `iced-x86`. Such a crate needs an `[[allow]]` entry with a reason, which keeps it visible as an informational finding. Maintainers may later replace the prefix rule with an explicit sub-crate list.

## Children

_Leaf: none._
