---
id: OUTR.DOCS.GUIDE.SITE
title: Static site /guide/ generator, browser WASI lab and scoped CSP
level: 4
parent: OUTR.DOCS.GUIDE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [site/src]
built_pr: 566
satisfies: [REQ-OUTR.DOCS.GUIDE-004]
covers: []
depends_on: [OUTR.DOCS.GUIDE.WASM, OUTR.DOCS.GUIDE.MAP, OUTR.DOCS.GUIDE.TOPICS]
uses_artifacts: [OUTR.SITE.GEN]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# OUTR.DOCS.GUIDE.SITE: Static site /guide/ generator, browser WASI lab and scoped CSP

## Purpose

Integrates the Executable Metrology Field Guide into `site/build.py`, `site/src/`, and `site/worker.js` so `metrologymaven.io/guide/` serves interactive WASM-powered metrology labs alongside live spec-tree field-map statistics, with zero third-party origins, graceful static fallbacks when JS/WASM is disabled, <500 KB HTML pages, and CSP `'wasm-unsafe-eval'` strictly scoped to `/guide/`.

## Scope

**In:**
- Navigation entry `Field Guide` (`/guide/`) and generated pages `/guide/` plus `/guide/<slug>/` in `site/build.py`
- Copying `target/wasm32-wasip1/release/fid-guide-wasm.wasm` (when built) into `site/dist/guide/fid-guide-wasm.wasm` with its SHA-256 digest surfaced in the lab status bar; clean fallback to kernel-verified static presets when the WASM artifact is absent or JS is off
- Zero-dependency same-origin browser WASI runner and interactive SVG lab controller (`site/src/guide-lab.js`) supporting live point dragging, outlier toggling, zone/datum inspection, Monte Carlo uncertainty budgets, ISO 14253 guardband risk curves, and RFC 8785 canonical JSON hashing
- Scoped Content-Security-Policy in `site/worker.js`: `/guide/*` serves `script-src 'self' 'wasm-unsafe-eval'`; all other routes preserve `script-src 'none'`
- Styling extensions in `site/src/style.css` preserving light/dark mode and high contrast

**Out:**
- Rust kernel math (`OUTR.DOCS.GUIDE.WASM`)
- Chapter JSON authoring (`OUTR.DOCS.GUIDE.TOPICS`)

## Requirements

- **REQ-OUTR.DOCS.GUIDE.SITE-001** `python3 site/build.py` shall generate `/guide/` and one page per topic in `guide/topics/*.json`, embedding live spec-tree domain counts in the Field Map and succeeding both with and without a prebuilt `fid-guide-wasm.wasm` artifact.
- **REQ-OUTR.DOCS.GUIDE.SITE-002** Every generated `/guide/` page shall make zero requests to third-party origins (`REQ-OUTR.SITE-007`), stay under the 500 KB HTML page budget (`REQ-OUTR.SITE-010`), use zero inline `<script>` blocks, and include complete static verified fallback tables/SVGs for no-JS environments.
- **REQ-OUTR.DOCS.GUIDE.SITE-003** `site/src/guide-lab.js` shall execute `fid-guide-wasm.wasm` via a minimal same-origin `wasi_snapshot_preview1` shim (`fd_read`, `fd_write`, `proc_exit`, `environ_get`, `environ_sizes_get`, `args_get`, `args_sizes_get`, `clock_time_get`, `random_get`) and update SVG visualizations, numeric readouts and SHA-256 `result_digest` live on user interaction.
- **REQ-OUTR.DOCS.GUIDE.SITE-004** `site/worker.js` shall serve `script-src 'self' 'wasm-unsafe-eval'` exclusively for `/guide/` routes and preserve `script-src 'none'` for all other routes.

## Acceptance criteria

- **AC-1** `python3 site/build.py` exits 0, builds `/guide/` and all 6 topic pages into `site/dist/guide/`, and every `.html` page is well under 500,000 bytes.
- **AC-2** The `site.yml` third-party-origin grep check and `tools/tests/test_guide.py` confirm zero external origins, zero inline `<script>` tags, valid static fallback readouts, and correct `worker.js` CSP scoping.
- **AC-3** When `target/wasm32-wasip1/release/fid-guide-wasm.wasm` is built and `python3 site/build.py` runs, `site/dist/guide/fid-guide-wasm.wasm` is copied and its SHA-256 digest matches the embedded badge on `/guide/` pages.
- **AC-4** Running the WASI shim in Node.js (or browser) against `site/dist/guide/fid-guide-wasm.wasm` executes all 7 kernel operations and produces the exact SHA-256 `result_digest` recorded in `guide/topics/*.json`.

## Design

- **Files:** `site/build.py`, `site/src/guide-lab.js`, `site/src/style.css`, `site/worker.js`, `tools/tests/test_guide.py`.
- **WASI browser runner:** Pure ES module / IIFE (`site/src/guide-lab.js`, zero external libraries) instantiating `WebAssembly.Module` once from `/guide/fid-guide-wasm.wasm` and spawning fresh deterministic instances per evaluation call (sub-millisecond instantiation, zero stale global state, identical to native CLI execution).
- **Determinism:** D1 (static build reproducible at a commit; WASM kernel outputs D0 bit-identical).

## Test fixtures

`guide/map/field-map.json`, `guide/topics/*.json`, and `target/wasm32-wasip1/release/fid-guide-wasm.wasm`.

## Build prompt

> Extend `site/build.py`, `site/src/style.css`, `site/src/guide-lab.js`, and `site/worker.js` to render `/guide/` and all six topic pages from `guide/map/field-map.json` and `guide/topics/*.json`, wiring live spec-tree counts from `scope.py` into the Field Map. When `target/wasm32-wasip1/release/fid-guide-wasm.wasm` exists, copy it to `site/dist/guide/fid-guide-wasm.wasm` and display its SHA-256 digest; otherwise build cleanly with verified static fallbacks. Implement a zero-dependency same-origin WASI host + interactive SVG sandbox widgets in `site/src/guide-lab.js` (no inline scripts, zero third-party requests), scope `script-src 'self' 'wasm-unsafe-eval'` exclusively to `/guide/` in `site/worker.js`, and verify AC-1..AC-4 in `tools/tests/test_guide.py` (including an end-to-end Node.js WebAssembly execution test of `fid-guide-wasm.wasm` through the exact browser WASI shim).

## Children

_Leaf: none._
