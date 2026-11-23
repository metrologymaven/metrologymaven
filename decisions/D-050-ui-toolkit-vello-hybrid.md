---
id: D-050
title: UI toolkit evaluation with Vello and Parley hybrid path
status: accepted
raised_by: founder
amends: [D-005]
blocks: [VIZ.SHELL]
decide_by: before confirming D-005 and starting VIZ.SHELL.DOCK.HOST
updated: 2026-11-22
---

# D-050: UI toolkit evaluation with Vello and Parley hybrid path

## Question

Should Metrology Maven add a focused GPU-2D comparison spike (`VIZ.SHELL.SPKVELLO`) evaluating **Vello + Parley + AccessKit** alongside the three `egui` spikes (`SPKTREE`, `SPKTABLE`, `SPKRPT`), and adopt a **hybrid UI architecture** (GPU compute 2D vector + shaped text for overlays, GD&T frames, charts, and the Report Designer canvas; `PanelUi`-isolated chrome) before locking D-005?

## Context

D-005 provisionally selected `egui + winit + wgpu`, confirmed only after three prototype spikes (`VIZ.SHELL.SPKTREE`, `VIZ.SHELL.SPKTABLE`, `VIZ.SHELL.SPKRPT`). Early results from building `SPKTREE` (`crates/fid-spike-ui/REPORT.md`) and D-044 expose structural limitations of pure immediate-mode `egui` for a dense, internationalised, accessible metrology suite:

1. **Accessibility tree gaps**: `egui` currently emits tree items without a `Tree` container role in AccessKit (`crates/fid-spike-ui/REPORT.md`, limitation #4) [unverified on egui >= 0.34].
2. **Text shaping, tabular numerals, and CJK IME**: `SPKRPT` requires multi-page WYSIWYG editing, fallback fonts, and CJK IME (`REQ-VIZ.SHELL.SPKRPT-004`), and `design/principles.md` (`P-DENSE`) requires tabular lining numerals everywhere. Immediate-mode CPU-tessellated `egui` lacks HarfBuzz-grade shaping and bidirectional/IME layout without heavy custom workarounds.
3. **Dependency and licence friction**: `egui 0.33` required scoped unmaintained-advisory exceptions (`D-044`: `ttf-parser`, `zvariant`, `zbus_names`, `zvariant_derive`) and disabling `default_fonts` because of `deny.toml`'s licence allowlist.
4. **Under-used GPU for 2D vector and report canvases**: CPU-tessellating complex report pages, 500-label overlays, SPC charts, and GD&T Feature Control Frames every frame leaves modern wgpu compute unused and complicates crisp 25%–400% zoom.
5. **Multi-monitor tear-off**: Metrology workstations routinely run dual 4K monitors (3D viewport on monitor 1; 500-row results table, SPC charts, and Proposal Inbox on monitor 2) sharing one `wgpu::Device` and VRAM chunk cache.

The `PanelUi` facade specified in `VIZ.SHELL.DOCK.HOST` already isolates panel crates from toolkit internals, so changing the 2D canvas or shell toolkit before `VIZ.SHELL.DOCK.HOST` lands has low blast radius.

## Options

| Option | For | Against |
|---|---|---|
| A. Pure `egui` (D-005 as written) | Simplest single-toolkit codebase; matches Craft family precedent | High risk of failing `SPKTABLE` (frozen columns + AccessKit table headers) and `SPKRPT` (CJK IME, rich text, 25%–400% zoom); CPU-tessellated 2D; ongoing advisory friction (D-044) |
| B. Full retained-mode Rust UI (`Masonry`/`Xilem` on `Vello + Parley`, or `Iced` with `cosmic-text`) | Retained AccessKit trees; first-class text shaping, tabular numerals, CJK IME; GPU compute 2D rendering via `wgpu`; native multi-window support | Younger docking/desktop widget ecosystems than `egui`; higher P1a bring-up effort for property grids and docking |
| C. **Hybrid (`Vello + Parley` GPU-2D canvas + `PanelUi`-isolated chrome)** (Recommended) | Immediately puts the hardest 2D surfaces (Report Designer canvas, 3D overlays/GD&T FCFs/MTSDF, SPC charts, inline tolerance sparkbars) on GPU compute (`Vello`) + `Parley` shaping; keeps P1a shell velocity if `SPKTREE`/`SPKTABLE` pass on `egui`, while providing a proven escape hatch to Option B via `PanelUi` if either spike fails | Two 2D rendering paths while `egui` remains for docked panels; requires vetting `Vello`/`Parley` crate licences and MSRV against `deny.toml` |

## Recommendation

Accept **Option C** by running `VIZ.SHELL.SPKVELLO` (benchmarking `Vello + Parley + AccessKit` on the Report Designer canvas, 500-label + GD&T overlay stress scene, and 10⁵-row virtualised table prototype, plus a `deny.toml` licence/advisory check and WASM/CPU sparse-strip fallback check) in parallel with hardware measurements of `SPKTREE`, `SPKTABLE`, and `SPKRPT`. See `research/viz-ux-sota/UI-TOOLKIT-BENCHMARK.md` for the exact benchmark protocol and pass/fail gates.

- If `SPKTREE` and `SPKTABLE` pass all criteria on H1 (including AccessKit `Tree` and `Table` container roles and p95 < 33 ms) while `SPKRPT` struggles on `egui`, confirm **Option C** (`egui` for docked panels behind `PanelUi`, `Vello + Parley` / GPU MTSDF for report canvas, overlays, and charts).
- If `SPKTREE` or `SPKTABLE` fails on `egui` and `SPKVELLO` passes, adopt **Option B** (retained-mode Rust UI on `Vello + Parley + AccessKit`) across `VIZ.SHELL` before implementing `VIZ.SHELL.DOCK.HOST`.

## Decision

**Accepted (option C), 2026-11-22, founder (via D-052).** Amends `D-005`: spike evaluation runs automatically on CI runners with headless `wgpu` fallback and automated AccessKit tree-role checks (no manual hardware/screen-reader gate). `spec/CONSTITUTION.md` §2 updated accordingly.
