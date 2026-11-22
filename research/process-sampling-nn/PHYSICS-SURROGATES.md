# Parametric Surrogate Physics-Error Generators

Companion to `research/process-sampling-nn/DESIGN.md` and `spec/cmm/strat/proc/NODE.md` (D-049).

## 1. Purpose

Large paired datasets of `(nominal CAD, process parameters, dense real-part deviation map)` are proprietary and often export-controlled. To pre-train the Process-Conditioned Geometric Neural Network (PC-GNN) at scale (500k-1M part-process pairs) without commercial FEA licenses, we use **fast, analytical/surrogate physics-error generators** (target: 2-5 CPU-seconds per part) applied to public B-rep CAD corpora (ABC Dataset, Fusion 360 Gallery, DeepCAD, plus procedurally generated prismatic, sheet and ribbed-housing parts).

Every generator produces a **continuous normal displacement field `delta(x, y, z)`** over the nominal B-rep surface, sampled across randomized process parameters so the network learns both the **expected spatial risk magnitude `mu(u, v)`** and the **run-to-run variance `sigma^2(u, v)`** plus the **spatial harmonic spectrum**.

> **Important:** Synthetic generators are for *pre-training* only. Acceptance of `CMM.STRAT.PROC` (`H-1` through `H-4`) is evaluated strictly on real physical parts (`BENCHMARK-PROTOCOL.md`), never on synthetic data alone.

## 2. Common output pipeline (per CAD part + process)

For each nominal B-rep part and sampled process family:
1. Extract face-adjacency graph `G = (V, E)` and evaluate each face on a `16 x 16` UV grid (plus a higher-resolution `64 x 64` evaluation grid for ground-truth FFT and form-error scoring).
2. Draw `K = 16` randomized realizations of process parameters (e.g., cutting force, chuck jaw phase, thermal gradient, shrinkage anisotropy) around the nominal process vector `p`.
3. Compute signed normal displacement `delta_k(u, v)` for each realization `k = 1..K`.
4. Compute supervision targets per UV cell `(u, v)`:
   - `mu(u, v) = mean_k |delta_k(u, v)|` (expected deviation magnitude / risk)
   - `sigma^2(u, v) = var_k(delta_k(u, v))` (process-parameter variance)
5. Compute per-face 16-bin spatial harmonic spectrum:
   - Cylindrical/conical/revolved faces: angular FFT of circumferential profiles (orders `k = 2..17`, excluding `k = 0` size and `k = 1` center shift).
   - Planar/freeform faces: radially binned 2D DCT/FFT power spectrum normalized to unit sum.
6. Compute optimal strategy labels by simulating candidate discrete hit counts and continuous scan paths on the `64 x 64` deformed surface and recording the smallest hit budget whose fitted form error is within 5% of ground truth.

## 3. Process-family surrogate models

### 3.1 Turning (`turning`)
Canonical frame: spindle axis = `+Z`, radial coordinate `r(x, y)`, azimuth `theta = atan2(y, x)`, axial coordinate `z` from chuck face (`z = 0`).

For cylindrical, conical and annular faces:
- **Chucking / jaw lobing:**
  `delta_lobe(theta, z) = sum_{m in {2, 3, 5, 7}} A_m * exp(-z / L_damp) * cos(m * theta + phi_m)`
  where `A_3` dominates for 3-jaw chucks, `A_2` for 2-jaw/thin-ring ovality, and `A_5, A_7` represent chatter harmonics.
- **Cantilever bending / taper (long unsupported overhang):**
  `delta_taper(z) = c_taper * (z / L_part)^3 / (r_eff^4 + eps)` (scaled by radial cutting force and part `L/D`).
- **Spindle runout / coaxiality drift:**
  `delta_runout(theta, z) = (e_0 + alpha_tilt * z) * cos(theta + phi_0)`.
- **Face dishing (planar end faces):**
  `delta_dish(r) = c_dish * (r / R_max)^2`.

### 3.2 3-Axis and 5-Axis Milling (`milling_3axis`, `milling_5axis`)
Canonical frame: primary spindle/tool axis = `+Z`.

- **Cantilever endmill deflection (pocket walls and internal corners):**
  For vertical/drafted walls at depth `d_pocket` below top rim and local concave corner curvature `kappa_corner`:
  `delta_tool(x) = c_tool * (F_cut * L_stickout^3 / (3 * E * I_tool)) * (1 + beta_corner * engagement(kappa_corner)) * (d_pocket / L_flute)^2`
  where `engagement` peaks at tight internal corner radii (`R_corner <= 1.2 * R_tool`).
- **Thin-wall plate deflection:**
  Using ray-cast local wall thickness `t(x)` and distance `h(x)` from the nearest stiffening base/rib edge:
  `delta_wall(x) = c_wall * (F_rad * h(x)^3) / (E_mat * t(x)^3 + eps)`.
- **Z-level step / tool-path scallop & chatter:**
  Periodic micro-waviness along Z or step-over direction with randomized spatial wavelength and amplitude proportional to tool overhang.

### 3.3 Injection Molding and Die Casting (`injection_molding`, `die_casting`)
Canonical frame: mold draw / pull axis = `+Z`, parting plane at `z = z_part`.

- **Anisotropic shrinkage & global warpage:**
  Smooth low-order quadratic/cubic polynomial field `delta_warp(x, y, z)` whose amplitude scales with bounding-box span and distance from the simulated gate location `x_gate`.
- **Differential shrinkage & sink marks opposite ribs/bosses:**
  Compute local wall thickness `t(x)` and thickness gradient `|grad t(x)|` via opposing-face ray casting; local sink displacement peaks where a perpendicular rib joins a nominal wall (`t_junction > 1.3 * t_nom`):
  `delta_sink(x) = -c_sink * max(0, t_junction(x) / t_nom - 1)^1.5 * exp(-dist_rib(x)^2 / (2 * w_rib^2))`.
- **Parting-line mismatch & draft distortion:**
  Localized lateral step/band centered on faces intersecting or near `z = z_part`, decaying exponentially with `|z - z_part|`.

### 3.4 Sheet Metal Stamping (`sheet_stamping`)
Canonical frame: punch/die stroke = `+Z`; bend lines detected from concave/convex cylindrical fillets with bend angle `alpha_bend`.

- **Elastic springback:**
  Each flange attached to a bend line rotates rigidly around the bend axis by `Delta_alpha = c_sb * (sigma_y / E_mat) * (R_bend / t_sheet) * alpha_bend`, producing linear displacement growing with distance from the bend line.
- **Flange twist / side-wall curl:**
  Quadratic cross-curvature term along free flange edges.
- **Near-bend hole distortion:**
  Any cylindrical hole whose center distance to a bend line is `< 3 * t_sheet` receives elliptical elongation perpendicular to the bend line.

### 3.5 Metal Additive Manufacturing — LPBF (`lpbf_additive`)
Canonical frame: build direction = `+Z`, build plate at `z = 0`.

- **Overhang sag (down-skin faces):**
  For faces where `n_z < -cos(45 deg)` (downward-facing angle beyond self-supporting threshold):
  `delta_overhang(x) = -c_oh * max(0, -n_z - 0.707)^2 * span_unsupported(x)`.
- **Thermal residual-stress curl:**
  Peaks near free upper edges (`z` high) and thin unsupported cantilevers, scaling inversely with local cross-sectional thickness `t(x)`.
- **Z-anisotropy & staircase roughness:**
  Slight global Z-scaling offset plus cosine-angle-dependent staircase ripple on sloped surfaces.

## 4. Domain randomization to reduce sim-to-real gap

To prevent the PC-GNN from overfitting clean analytical formulas, every training sample applies:
- Random process-parameter perturbation (+/- 30-50% log-uniform amplitudes, random harmonic phases, random gate/clamp/fixture positions).
- Smooth correlated Gaussian Random Field (GRF) residual noise (`20-35%` of signal energy) representing unmodeled shop-floor effects (fixture seating error, ambient thermal drift, tool wear).
- Missing-parameter dropout (randomly masking 30-60% of optional process scalars so inference stays robust when a shop supplies only the process family and material).
