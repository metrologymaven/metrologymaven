# Intuitive Metrology UX & Spatial Interaction Specification

Companion UX specification for the upgrades integrated across `VIZ.OVERLAY`, `VIZ.VIEW`, `VIZ.INBOX`, `VIZ.SHELL`, `VIZ.KIOSK`, and `UX.SCREENS`. Every interaction obeys `design/principles.md` (`P-DENSE`, `P-DATA`, `P-EXPLAIN`, `P-KEYS`, `P-FAMILIAR`, `P-NOCOLOR`) and **CONSTITUTION C1/C2/C3/C5/C6/C9** (no incumbent trade dress; every mutation commits through the registry command bus).

## 1. Direct 3D Manipulation & Contextual Action Strip

**Problem**: Schema-generated forms (`VIZ.SHELL.PALETTE.FORMS`) guarantee completeness and keyboard access, but forcing metrologists to round-trip to panels or a 500-command palette for routine 3D actions slows inspection down.

**Design**:
- **Interactive 3D Gizmos**: Clip planes (`VIZ.VIEW.CLIP`), section planes (`SCAN.SECT`), datum targets, and comparison-point normals display direct-manipulation handles in 3D with magnetic snapping (axis/face/step increments), live HUD dimension/angle readouts, and live GPU preview (`R1`). Releasing the drag (or pressing `Enter`) commits **one** journaled registry command (`W2`). Pressing `Esc` cancels cleanly with zero journal side effects.
- **Contextual Selection Action Strip**: Selecting a geometric entity in the 3D viewport anchors a compact, quiet, non-blocking strip near the selection centroid (repositioned automatically to stay clear of labels and viewport edges):
  ```text
  [1 Fit Cylinder] [2 Set Datum] [3 Cylindricity] [4 Section Cut] [5 Compare] [···]
  ```
  - Backed strictly by the **same registry commands** filtered by selection type (zero side doors, C3).
  - Number keys `1`–`5` invoke actions immediately when the viewport has focus; `···` opens the command palette pre-filtered to the active selection.

## 2. Histogram-Embedded Scrubbable Colorbar (`VIZ.OVERLAY.LEGEND` & `SCALEED`)

**Problem**: Static numeric legends hide how deviations actually cluster around tolerance limits.

**Design**:
```text
   +0.500 mm ┤ ████          (1.2%)
   +0.300 mm ┤ ███████       (4.8%)  ◄── draggable USL / band handle
   +0.100 mm ┤ █████████████ (28.4%)
    0.000 mm ┤ ████████████████ (34.1%)
   -0.100 mm ┤ ███████████   (25.9%)
   -0.300 mm ┤ █████         (4.5%)  ◄── draggable LSL / band handle
   -0.500 mm ┤ ██            (1.1%)
```
- **Embedded Kernel Histogram**: Renders horizontal distribution bars beside each bin directly from the kernel legend's bin counts/percentages (zero UI recomputation, `REQ-VIZ.OVERLAY-002`).
- **Hover-to-Isolate**: Hovering any bin row dims all non-matching points/triangles in 3D (display-only shader mask) so out-of-tolerance hot spots pop out immediately.
- **Direct Handle Scrubbing**: Dragging a bin edge or tolerance handle updates the display via live GPU preview (marked `PREVIEW`) and commits exactly one `view.set_colormap@1` + kernel re-bin on release; exact decimal keyboard entry remains one click or keystroke away.

## 3. Inline Tolerance + Uncertainty Sparkbars (`VIZ.SHELL.TREE.RESULTS`)

**Problem**: Metrology Maven's honest uncertainty (`UEC-0..3` and guard-banded `INDETERMINATE`) can get buried inside 14-column tables.

**Design**:
Each row of the characteristic/results table includes a compact inline visual bar alongside verbatim kernel-formatted tabular numerals (`tnum`, `zero`):
```text
Balloon | Characteristic | Actual    | Dev       | Tolerance & Uncertainty (U)     | Status
   14   | ⌖ Pos Ø0.100   |  0.0782   | +0.0782   | [ ───────|──(●)──| ] UEC-2     | ⚠ INDETERMINATE
   15   | ⏥ Flat 0.050   |  0.0141   | +0.0141   | [ ──(●)──────────| ] UEC-2     | ✓ CONFORMS
   16   | ⌀ Dia 12.000   | 12.0624   | +0.0624   | [ ───────────────|───(●) ]     | ✕ NONCONFORMS
```
- Bar geometry is computed purely from kernel-supplied f64 `(nominal, lsl, usl, actual, U)` for display; numbers remain verbatim kernel formatter strings.
- Respects `P-NOCOLOR`: uses tick marks for limits, bracketed whiskers `(●)` for expanded uncertainty `±U`, and distinct glyphs + text labels for status.
- In 3D deviation maps (`VIZ.RENDER.LOD.ATTR`), an optional **Uncertainty Guard-Band Hatch** overlays a fine diagonal hatch on points/triangles where `|deviation − limit| < U`.

## 4. 3D Tolerance-Zone Ghosts & Normal-Displacement Exaggeration

- **Tolerance-Zone Ghosts (`VIZ.OVERLAY.GDTGLYPH.PLACE`)**: Hovering a GD&T Feature Control Frame or a row in the Results table projects the **actual 3D tolerance zone** (coaxial cylinder for Position/Concentricity, parallel planes for Flatness/Parallelism/Perpendicularity, offset envelope for Surface Profile) plus the active **Datum Reference Frame triad (`A|B|C`)** with constrained DOF badges (`Tx Ty Tz Rx Ry Rz`).
- **Deviation Exaggeration (`VIZ.RENDER.SHADE.MESH`)**: A display-only normal-displacement multiplier (`1×` to `500×`) in the viewport HUD lets engineers visually inspect micrometre-scale warpage on large parts. Always renders an unmistakable `EXAGGERATED N× (DISPLAY ONLY)` chip and disables picking against displaced vertices (picking always resolves against true f64 geometry).

## 5. Proposal Inbox Spatial Wipe Curtain & Visual Dependency-Graph Lens

- **Spatial Wipe Curtain (`VIZ.INBOX.DIFF.VISUAL`)**: In addition to before/after toggle and side-by-side split (`VIZ.VIEW.MULTI`), reviewers can drag a vertical/horizontal **wipe curtain** directly across the 3D viewport (left = working document head; right = agent proposal head) or enable **Delta-Only Heatmap** mode (hiding unchanged geometry and colouring only entities modified by the proposal).
- **Visual Dependency-Graph Lens (`VIZ.SHELL.DOCK.HOST`)**: A toggleable, read-only visual DAG overlay/panel (`CAD + Scan → Alignment → Features → Datums → GD&T → Report`) driven directly from `CORE` dependency-graph edges and `VIZ.RENDER.SCENE.STALE` flags. Clicking any node highlights its 3D geometry and opens its "Why?" provenance panel (`VIZ.SHELL.TREE.WHY`), making stale propagation immediately legible.

## 6. Viewport Ergonomics, Multi-Monitor & Shop-Floor Guidance

- **Cursor-Pivot Orbit + Orthographic-Default View Cube (`VIZ.VIEW.CAMERA.NAV`)**:
  - Orbit pivot automatically locks onto the 3D surface point under the cursor (via async depth/ID readback), preventing camera fly-away when inspecting tiny features on large parts.
  - A quiet corner **Orientation Cube** reflects the active kernel DRF axes (`X/Y/Z`), lets users click faces/edges/corners for standard views, and provides a one-click **Ortho / Persp** toggle (defaulting to **Orthographic** for dimensional inspection and **Perspective** for large-volume network views).
- **Zero-Chrome Focus Mode (`Tab` / `F11`)**: Hides docked chrome, leaving > 95% of the screen to the 3D viewport with compact translucent HUD chips (Legend, Active DRF, Selection Readout, Filter pill).
- **Multi-Monitor Tear-Off (`VIZ.SHELL.DOCK.HOST`)**: Floating panels detach into OS-native secondary windows sharing **one `wgpu::Device` and one VRAM chunk cache**, with dirty-driven viewport texture caching so UI edits on monitor 2 never re-render static 10⁸-point clouds on monitor 1.
- **Shop-Floor Live Coverage & Guidance (`VIZ.KIOSK.STATUS` / live scanning)**: During live scanning or probing cycles, renders a display-only 3D **scan-coverage map** (scanned vs unscanned nominal faces) and **next-probe-point approach arrows** alongside the ≥ 20 mm gloved-touch conformance status display.
