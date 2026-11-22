---
id: VIZ.SHELL.SPKVELLO
title: Toolkit spike - Vello and Parley GPU-2D canvas and hybrid benchmark
level: 3
parent: VIZ.SHELL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-003, REQ-VIZ.SHELL-004, REQ-VIZ.SHELL-009]
covers: []
depends_on: []
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# VIZ.SHELL.SPKVELLO: Toolkit spike - Vello and Parley GPU-2D canvas and hybrid benchmark

## Purpose

The fourth toolkit spike proposed in **D-050** (amending D-005): evaluate **Vello + Parley + AccessKit** on `wgpu` for the hardest 2D and dense-data surfaces (multi-page Report Designer canvas, 500-label + GD&T Feature Control Frame overlays, 10⁵-row characteristic table with inline tolerance sparkbars, and two-window shared-`wgpu::Device` tear-off), plus a WebGL2/CPU sparse-strip fallback and `cargo deny` hygiene check. Its report gives maintainers measured hardware evidence to confirm D-005/D-050 (pure `egui`, full retained GPU-2D, or hybrid behind `PanelUi`).

## Scope

**In:** throwaway prototype crate/example benchmarking Vello compute 2D + Parley text shaping + AccessKit retained tree across five workloads (`research/viz-ux-sota/UI-TOOLKIT-BENCHMARK.md`): (W1) 5-page 200-block report canvas at 25%–400% zoom with Japanese IME and tabular numerals; (W2) 500 labels + 100 GD&T FCFs + histogram colorbar over a live `wgpu` 10⁶-point viewport; (W3) 10⁵-row virtualised table with frozen columns, inline tolerance+uncertainty sparkbars and AccessKit `Table`/`RowHeader`/`ColumnHeader` roles; (W4) two OS-native windows sharing one `wgpu::Device` with dirty-driven viewport caching; (W5) CPU/sparse-strip fallback and `cargo deny check` licence/advisory audit; results report (`REPORT-VELLO.md` and `timings.csv`) with both CPU wall time and GPU `TIMESTAMP_QUERY` pass timings.
**Out:** production shell (`VIZ.SHELL.DOCK`), production report designer (`RPT.DESIGN`), adopting any crate in product layers, deciding D-050 (maintainers).

## Requirements

- **REQ-VIZ.SHELL.SPKVELLO-001** The spike shall render the 5-page 200-block report canvas (W1) and 500-label + 100-FCF overlay scene (W2) via GPU compute 2D (`Vello`) and shaped text (`Parley`) with tabular lining numerals and CJK IME composition, measuring both CPU wall time and GPU pass time.
- **REQ-VIZ.SHELL.SPKVELLO-002** The spike shall pass the latency gate only if p95 frame time stays under 33 ms (stretch target 16.6 ms) on the PLAT H1 profile across W1, W2 and the 10⁵-row virtualised table (W3), and the overlay pass stays under 2 ms at 1920 × 1080 [unverified].
- **REQ-VIZ.SHELL.SPKVELLO-003** The spike shall expose retained AccessKit container roles (`Tree`, `Table`, `RowHeader`, `ColumnHeader`) verified manually with Windows Narrator or NVDA.
- **REQ-VIZ.SHELL.SPKVELLO-004** The spike shall demonstrate two OS-native windows sharing one `wgpu::Device` and one point-cloud GPU buffer with zero 3D re-render when only UI state changes in the second window.
- **REQ-VIZ.SHELL.SPKVELLO-005** The spike shall test a non-compute fallback (Vello CPU/sparse-strip or software renderer) and publish a `cargo deny` licence/advisory table plus pass/fail per criterion in `REPORT-VELLO.md`.

## Acceptance criteria

- **AC-1** Given fixtures `fixtures/viz/report-canvas-200`, `fixtures/viz/char-table-100k` and `fixtures/viz/labels/dense500` on H1, when `--bench` runs W1–W3, then `REPORT-VELLO.md` records both CPU p95 and GPU `TIMESTAMP_QUERY` p95 against 33 ms / 16.6 ms and 2 ms overlay targets.
- **AC-2** Given W1 with a Japanese IME on Windows composing "そくてい", then the committed string matches verbatim with fallback font glyphs rendered cleanly at 25%–400% zoom (manual step recorded in report).
- **AC-3** Given W3 with NVDA or Narrator, then focused cells announce table container, row/column headers, tabular numeric value and status text (manual step recorded in report).
- **AC-4** Given W4 (two windows, 10⁶-point buffer), when scrolling the table in window 2 while the camera in window 1 is idle, then GPU buffer count equals 1 and 3D render pass count during table scroll equals 0.
- **AC-5** Given `cargo deny check` on the spike graph, then `REPORT-VELLO.md` lists every licence and advisory finding with zero hidden exceptions.

## Design

- **Registry command(s):** none (throwaway prototype).
- **Data structures:** reuses synthetic tree/table/report models from `fid-spike-ui` with a `Vello + Parley + AccessKit` renderer harness and a `wgpu::QuerySet` GPU timestamp collector.
- **Algorithm:** render 3D viewport into a cached `Rgba16Float` texture on dirty events only; compose GPU-2D scene (`vello::Scene`) over the cached texture via shared `wgpu::Device`; evaluate CPU sparse-strip fallback on T2/headless adapters.
- **Errors:** typed setup diagnostics (`NoAdapter`, `TimestampUnsupported`, `FallbackUsed`).
- **Determinism:** N (observed timings); fallback render checked against golden tolerance.
- **Performance budget:** W1–W3 p95 < 33 ms (stretch < 16.6 ms); W2 overlay < 2 ms on H1 [unverified].
- **Crate:** `fid-spike-ui` (example `vellobench`; throwaway, dev-dependencies only, never linked by product crates).

## Test fixtures

`fixtures/viz/report-canvas-200`, `fixtures/viz/char-table-100k`, `fixtures/viz/labels/dense500` (all generated, in-repo).

## Build prompt

> In throwaway crate `fid-spike-ui`, add example `vellobench` implementing the D-050 GPU-2D benchmark protocol (`research/viz-ux-sota/UI-TOOLKIT-BENCHMARK.md`) using `Vello + Parley + AccessKit` over `wgpu` (dev-dependencies only, never linked by product crates). Implement workloads W1 (5-page 200-block report canvas, 25%–400% zoom, tabular numerals, CJK IME), W2 (500 labels + 100 GD&T FCFs + histogram colorbar over a 10⁶-point `wgpu` viewport), W3 (10⁵-row virtualised table with frozen columns, inline tolerance+uncertainty sparkbars and AccessKit `Table`/`RowHeader`/`ColumnHeader` roles), W4 (two OS-native windows sharing one `wgpu::Device` with dirty-driven viewport caching and zero 3D re-renders while scrolling window 2), and W5 (non-compute fallback + `cargo deny` report). Measure both CPU wall time and GPU `TIMESTAMP_QUERY` pass timings and write `REPORT-VELLO.md` and `timings.csv` with pass/fail per criterion. Rules: CONSTITUTION §8 (no panics/unsafe); never adopt a toolkit in product crates; input to D-005/D-050 maintainers only.

## Open questions

- Governed by open decision **D-050**; does not block P1a `SPKTREE`/`SPKTABLE`/`SPKRPT` measurements and runs in parallel so D-005 is confirmed with comparative data.

## Children

_Leaf: none._
