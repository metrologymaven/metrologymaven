# UI Toolkit & GPU-2D Benchmark Protocol (`VIZ.SHELL.SPKVELLO` & D-050)

Companion evaluation protocol for **D-050** (`decisions/D-050-ui-toolkit-vello-hybrid.md`). Extends the D-005 spike suite (`SPKTREE`, `SPKTABLE`, `SPKRPT`) with a fourth spike (`VIZ.SHELL.SPKVELLO`) evaluating **Vello + Parley + AccessKit** so the D-005 decision is made on measured hardware evidence rather than assumptions.

## 1. Why a Fourth Spike Is Needed Before Locking D-005

`crates/fid-spike-ui/REPORT.md` and `decisions/D-044-scoped-unmaintained-advisory-exception-egui-spike.md` identify four structural risks in pure immediate-mode `egui`:

| Risk Area | Current `egui 0.33` Status | Metrology Maven Requirement |
|---|---|---|
| Accessibility container roles | Tree items lack a `Tree` parent container role (`REPORT.md` #4) | `REQ-VIZ.SHELL-004`, `REQ-VIZ.SHELL.SPKTABLE-004` (Windows Narrator/NVDA + macOS VoiceOver + Linux AT-SPI) |
| Complex text shaping & CJK IME | Limited shaping; fallback fonts & IME fragile at arbitrary zoom | `REQ-VIZ.SHELL.SPKRPT-004` (Japanese IME composition, fallback fonts), `P-DENSE` tabular lining numerals |
| GPU 2D vector throughput | Full CPU tessellation per frame (`epaint`) | 200-block 5-page report canvas at 25%–400% zoom, 500 3D labels, GD&T FCFs, live SPC charts, inline table sparkbars |
| Dependency & licence hygiene | 4 unmaintained advisories (`D-044`), default fonts blocked by OFL licence gate | `CONSTITUTION §8`, `deny.toml` zero-exception steady state |

## 2. Architectures Under Comparison

1. **Baseline (D-005 Option A)**: Pure `egui + winit + wgpu` across all screens.
2. **Full Retained GPU-2D (D-005 Option B)**: Retained-mode Rust UI (`Masonry`/`Xilem` on `Vello + Parley + AccessKit`, or `Iced` with `wgpu + cosmic-text`).
3. **Hybrid (D-050 Option C — Recommended)**:
   - **GPU-2D Canvas Layer (`Vello + Parley`)**: Report Designer canvas (`RPT.DESIGN`), 3D annotation/GD&T FCFs/MTSDF overlays (`fid-overlay`), SPC charts, histogram colorbars, and inline table tolerance sparkbars.
   - **Docked Shell Chrome (`PanelUi` facade)**: `egui` for P1a velocity *only if* `SPKTREE` and `SPKTABLE` pass all H1 latency + AccessKit criteria; otherwise migrate the chrome behind `PanelUi` to Option B with zero changes to panel domain crates.

## 3. Benchmark Workloads (`VIZ.SHELL.SPKVELLO`)

All workloads run on the **PLAT H1 reference profile** (integrated GPU, vsync off for raw timing + vsync on for frame-pacing jitter) and record **both CPU wall time and GPU pass time (`TIMESTAMP_QUERY`)**:

| Workload | Scale | Pass Threshold |
|---|---|---|
| **W1: Report Designer Canvas** | 5 A4 pages, 200 blocks (tables, charts, GD&T blocks, 1 live `wgpu` 3D viewport), drag + zoom 25%–400% + Japanese IME typing | p95 CPU + GPU frame time `< 16.6 ms` (60 fps target; hard gate `< 33 ms`) [unverified] |
| **W2: Dense 3D Overlay & GD&T FCFs** | 500 labels with leader lines + 100 multi-compartment GD&T Feature Control Frames + histogram colorbar over a 10⁶-point viewport | Overlay GPU pass `< 2 ms` at 1920×1080; zero visual blur at 75%–200% UI scale [unverified] |
| **W3: 10⁵-Row Characteristic Table** | 10⁵ rows × 14 columns, 2 frozen columns, inline tolerance+uncertainty sparkbars, tabular numerals, live filter + sort | p95 `< 16.6 ms` (hard gate `< 33 ms`); NVDA/Narrator announces `Table`, `RowHeader`, `ColumnHeader`, value, and status text |
| **W4: Multi-Window Shared Device** | Two OS-native windows (3D viewport on window 1; virtualised table + chart on window 2) sharing one `wgpu::Device` | Zero VRAM duplication of point/mesh buffers; idle UI frame skips 3D re-render (`0 ms` 3D GPU pass) |
| **W5: Fallback & Hygiene Gate** | WebGL2 / CPU sparse-strip fallback render; `cargo deny check` across all targets | Fallback renders within golden tolerance; 0 licence violations and 0 unapproved advisories |

## 4. Decision Matrix for Maintainers

- **If `SPKTREE` + `SPKTABLE` PASS and `SPKVELLO` PASSES**: Confirm **Option C (Hybrid)** — ship P1a shell fast behind `PanelUi` while building `RPT.DESIGN` and `fid-overlay` directly on `Vello + Parley` / GPU MTSDF.
- **If `SPKTREE` or `SPKTABLE` FAILS (e.g., AccessKit container roles or table latency) and `SPKVELLO` PASSES**: Adopt **Option B** (full retained GPU-2D Rust UI) before `VIZ.SHELL.DOCK.HOST` is implemented.
- **If `Vello` fails `deny.toml` or T2/WebGL2 fallback**: Keep `Parley` + custom `wgpu` MTSDF/SDF instanced shaders in `fid-overlay` (zero extra heavy dependencies) and re-evaluate `Iced` vs `egui` for `VIZ.SHELL`.
