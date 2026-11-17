---
id: VIZ.WEB.BUILD
title: WASM build and no-kernel CI guards
level: 3
parent: VIZ.WEB
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.WEB-001, REQ-VIZ.WEB-002]
covers: []
depends_on: [VIZ.RENDER.FRAME]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.WEB.BUILD: WASM build and no-kernel CI guards

## Purpose

Build the browser viewer (`fid-viewer-lite`) for wasm32 from the same render, scene, overlay and view crates as the desktop, and enforce in CI that (a) no metrology kernel or CAD kernel crate is linked, (b) rendering logic has no browser-only fork, and (c) the bundle stays within its size budget.

## Scope

**In:** the wasm32 build target and its CI job; a crate-graph denylist check; a fork check on rendering crates; bundle-size measurement; a smoke test that the bundle loads in a headless browser.
**Out:** backend fallback behaviour (`VIZ.WEB.FALLBACK`); package format and loader (`VIZ.WEB.PKG`, `VIZ.WEB.LOADER`); supported browsers (`PLAT`); hosting (`OUTR`).

## Requirements

- **REQ-VIZ.WEB.BUILD-001** CI shall build `fid-viewer-lite` for wasm32 from the shared render, scene, overlay and view crates on every change to those crates.
- **REQ-VIZ.WEB.BUILD-002** CI shall fail if the wasm32 crate graph contains any crate on the kernel denylist (metrology kernel and fit crates) or any CAD-kernel binding.
- **REQ-VIZ.WEB.BUILD-003** CI shall fail if a shared rendering crate contains wasm-only conditional code outside an allow-listed backend-initialisation module.
- **REQ-VIZ.WEB.BUILD-004** CI shall report the compressed bundle size and fail if it exceeds 5 MB gzip [unverified].
- **REQ-VIZ.WEB.BUILD-005** CI shall load the built bundle in a headless browser and fail if initialisation reports an error.

## Acceptance criteria

- **AC-1** Given the workspace at a clean commit, when the CI job `web-build` runs, then it produces `fid_viewer_lite.wasm` and the JS loader, and reports the gzip size.
- **AC-2** (guard) Given a test branch that adds a dependency on a kernel crate to `fid-overlay`, when `web-build` runs, then it fails naming the denylisted crate and the path through the graph.
- **AC-3** (guard) Given a test branch that adds `#[cfg(target_arch = "wasm32")]` drawing code to a pass module outside the allow-listed module, then the fork check fails naming the file and line.
- **AC-4** Given fixture `fixtures/viz/web-smoke` (tiny package), when the headless-browser smoke test runs, then initialisation succeeds and reports a capability tier.
- **AC-5** Given a bundle larger than the budget (test by lowering the budget), then the job fails with the measured size.

## Design

- **Registry command(s):** none (build and CI task).
- **Data structures:** `web-guards.toml` holding the kernel denylist (crate name patterns), the fork allow-list (module paths) and the size budget.
- **Algorithm:** `cargo metadata` for the wasm32 target, walk the resolved graph, match denylist patterns, print the shortest path to any hit; scan shared rendering crates' sources for wasm-target `cfg` attributes outside allow-listed modules; build with size-optimised profile, gzip, compare to budget; run the smoke page in a headless browser.
- **Errors:** CI failures with named crates, files or sizes.
- **Determinism:** D0 (graph and source checks are deterministic).
- **Performance budget:** CI job under 10 min [unverified].
- **Crate:** `fid-viewer-lite` (build target) plus an `xtask` subcommand `web-guards`.

## Test fixtures

`fixtures/viz/web-smoke`: a tiny generated ViewerPackage (one mesh, one NamedView); in-repo. Guard tests use throwaway branches created by the test harness, not committed code.

## Build prompt

> Add a wasm32 build of crate `fid-viewer-lite` that links only the shared render, scene, overlay and view crates, and an `xtask web-guards` command run in CI. The guard reads `web-guards.toml`: (1) walk the wasm32 crate graph from `cargo metadata` and fail, printing the dependency path, if any crate matches the kernel denylist (metrology kernel and fit crates) or a CAD-kernel binding; (2) scan shared rendering crates for wasm-target `cfg` code outside allow-listed backend-initialisation modules and fail with file and line; (3) build with a size-optimised profile, gzip the bundle and fail above 5 MB; (4) load the bundle in a headless browser on fixture `web-smoke` and fail on initialisation errors. Tests: clean build passes; injected kernel dependency, injected wasm fork and lowered size budget each fail with a precise message; smoke test reports a tier. CI changes go in their own PR (AGENTS.md). Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- If readouts need a formatting-only port of the kernel formatter (parent open question), it must be published by KRN as a separate crate with no evaluation code, and added to an explicit allow-list here.

## Children

_Leaf: none._
