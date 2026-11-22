# Process-Conditioned Geometric Neural Network (PC-GNN) Design

Spec node: `CMM.STRAT.PROC` (blocked on `decisions/D-049-process-aware-sampling-nn.md`).

## 1. Objective and architectural boundary

Traditional CMM program generators treat CAD geometry as process-blind, placing hits uniformly or by nominal curvature alone. Real manufactured parts exhibit spatial geometric errors governed by the manufacturing process that produced them (chucking lobing in turning, cantilever tool and thin-wall deflection in milling, shrinkage warpage and sink marks in molding, bend springback in stamping, overhang sag and thermal curl in metal additive manufacturing).

The **Process-Conditioned Geometric Neural Network (PC-GNN)** predicts where geometric deviation risk and spatial harmonics peak on a nominal B-rep CAD model given how the part was made. It operates strictly inside Metrology Maven's neuro-symbolic safety boundary:

1. **Propose (PC-GNN, `CMM.STRAT.PROC`):** Predicts per-face deviation risk (`mu`, `sigma^2`), spatial frequency / harmonic priors, and draft `MeasurementStrategy` / `ScanStrategy` proposals tagged `agent_authored: true`.
2. **Verify (Deterministic Rust):** `CMM.STRAT.CHECK` enforces minimum-point and non-degeneracy rules; `CMM.COLL`, `CMM.PATH` and `CMM.SIM` solve probe accessibility and prove collision-free motion for the specific target CMM; `CMM.GEN` emits a byte-deterministic draft Command Program (or QIF / DMIS / I++ DME).
3. **Approve (Human Gate `H1`):** A person approves the simulated program and motion envelope once, followed by a mandatory reduced-speed setup run.
4. **Evaluate (Deterministic Kernel `KRN`, `GDT`, `UNC`):** The neural network never computes a fit, a value of record, a conformance verdict, or an official reported measurement uncertainty (`U`).

## 2. Two-layer hierarchy: process physics + exact-machine adapters

- **Layer 1 — Base Process-Family Model (open, day-one generalization):**
  Trained on physics-based parametric error generators and real-part benchmarks across process families (`turning`, `milling_3axis`, `milling_5axis`, `injection_molding`, `die_casting`, `sheet_stamping`, `lpbf_additive`, `grinding`, `edm`). Ships as a pinned, ~30M-parameter ONNX file (~35 MB INT8 / ~60 MB FP16) that runs offline on any shop-floor CPU.
- **Layer 2 — Exact-Machine / Mold-Cavity Adapter (optional `ENT` on-prem add-on):**
  Real machines and mold cavities carry unique systematic fingerprints (axis backlash, spindle thermal drift, specific cavity warpage). Instead of retraining the base model or uploading proprietary CAD to a cloud service, a self-hosted `fid server` attaches a compact **LoRA adapter (~1-2 MB)** per machine or cavity, fine-tuned strictly on-premises from that shop's own historical inspection runs (including sparse touch-trigger runs via masked coordinate loss). Sudden residual spikes against the learned adapter provide fleet drift alerts.

## 3. Input representation

### 3.1 B-rep face-adjacency graph `G = (V, E)`
Extracted from `GEO.CAD` (STEP AP242 / QIF 3.0 B-rep):

- **Face node `v_i`:**
  - **UV-grid tensor `(16 x 16 x 7)`:** normalized 3D coordinates `(x, y, z)`, unit outward surface normal `(n_x, n_y, n_z)`, and trim-mask bit `m in {0, 1}`.
  - **Analytical face scalars (16-d):** one-hot surface class (plane, cylinder, cone, torus, sphere, NURBS), log-area, mean/Gaussian curvature stats, aspect ratio, local minimum wall thickness (ray-cast distance to opposing face), hole depth-to-diameter ratio (`L/D`), and overhang angle relative to primary process axis.
  - **Tolerance & accuracy context (6-d):** GD&T characteristic class (form, orientation, location, profile), normalized tolerance band `T`, target CMM rated accuracy `MPE_E`, and ratio `MPE_E / T` (with missing-value mask when planning without PMI).
- **Edge `e_ij` (shared B-rep edge):**
  - **Curve-grid tensor `(16 x 6)`:** 3D curve points and unit tangents.
  - **Edge scalars (6-d):** dihedral angle (convex / concave / tangent), normalized edge length, fillet radius indicator.

### 3.2 Manufacturing process conditioning vector `p`
- Process family embedding (`turning`, `milling_3axis`, `milling_5axis`, `injection_molding`, `die_casting`, `sheet_stamping`, `lpbf_additive`, `grinding`, `edm`).
- Material stiffness/thermal class (aluminum, steel, titanium, nickel superalloy, engineering polymer, sheet steel).
- Workholding / fixture class (3-jaw chuck, collet, vise, vacuum plate, clamp plate, build plate, unrestrained).
- Primary process axis `d_hat` in part frame (spindle axis, tool axis, draw/parting direction, build `+Z`).
- Optional continuous scalars (nominal cutter/nozzle diameter, nominal wall thickness, chuck jaw count) paired with binary presence masks.

### 3.3 Frame canonicalization
Parts are centered and scaled by bounding-box diagonal and rotated so the declared primary process axis aligns with `+Z`. Training applies random rotations around `+Z` to enforce azimuthal invariance.

## 4. Network architecture (~25M-40M parameters)

```text
[Face UV Grid 16x16x7] ──► 2D ResNet-Tiny (4 blocks) ──► 256-d ──┐
[Face Scalars + GDT]   ──► 2-layer MLP               ──►  64-d ──┼─► Concat (320-d)
                                                                 │
[Edge Curve 16x6]      ──► 1D ResNet-Tiny (3 blocks) ──► 128-d ──┤
                                                                 ▼
[Process Vector p] ──► Process MLP ──► FiLM (gamma, beta) at every layer ──► 6-Layer Edge-Conditioned
                                                                             Graph Transformer / GATv2
                                                                             (hidden=384, 8 heads)
                                                                 │
        ┌────────────────────────────────────────────────────────┼──────────────────────────────┐
        ▼                                                        ▼                              ▼
[Head 1: UV Risk Decoder]                           [Head 2: Spectral Head]        [Head 3: Strategy Prior]
2D U-Net / Transposed CNN                           2-layer MLP                    2-layer MLP
(skip-connected from Face CNN)                      -> 16 spatial harmonic bins    -> discrete vs scan score,
-> 16x16x2 (mu, log sigma^2)                        (lobe / waviness spectrum)        hit density, filter class
```

### Training loss
- **Head 1 (Risk map):** Masked heteroscedastic Gaussian negative log-likelihood (NLL) over valid trim-mask cells:
  `L_risk = mean_valid( 0.5 * exp(-s) * (y - mu)^2 + 0.5 * s )`, where `s = log(sigma^2)`.
- **Head 2 (Harmonic spectrum):** KL divergence / MSE against ground-truth normalized angular/spatial FFT magnitudes on each face.
- **Head 3 (Strategy prior):** Cross-entropy for discrete vs. continuous scan recommendation + smooth L1 loss against optimal minimum hit count that achieves <= 5% form-error underestimation in ground-truth simulation.

## 5. Uncertainty and accuracy integration

1. **Tolerance-to-accuracy sizing (`MPE_E / T`):** Tight-tolerance features where `MPE_E / T` is large (small gauging ratio) receive increased point density or continuous analog scanning (when `CMM.PROBE.SENSOR` supports scanning) to keep sampling uncertainty low relative to ISO 14253-1 guardbands.
2. **Predictive variance (`sigma^2`) fallback:** If a face's mean or 90th-percentile predicted `sigma^2` exceeds the calibrated OOD threshold, the deterministic strategy compiler ignores sparse risk weighting and emits conservative uniform/dense sampling with an explicit fallback note in the proposal brief.
3. **Strict separation from official `U`:** Official measurement uncertainty `U` is computed exclusively by `UNC` after physical measurement.

## 6. Compute, runtime and cost estimates

All hardware timings and cloud pricing figures below are engineering estimates `[unverified]`:

| Stage | Workload | Hardware | Estimated wall time | Estimated cost (compute) |
|---|---|---|---|---|
| Synthetic corpus generation | 500k-1M `(part, process)` pairs (B-rep UV sampling + surrogate physics) | 64-128 vCPU spot instances + 500 GB object storage | 24-48 hours | $150 - $500 `[unverified]` |
| Narrow v0 prototype | 50k parts, 2 processes (milling + turning) | 1x RTX 4090 / L40S / A100 | 8-16 hours/run (3-5 days R&D) | $200 - $600 `[unverified]` |
| Full v1 multi-process model | 500k-1M pairs, 6-8 processes, ~10-15 ablations | 4x-8x H100 (80 GB) | 12-24 hours/run (~1,500-2,500 H100-hrs total) | $3,000 - $7,500 `[unverified]` |
| Real-part validation benchmark | 50-200 real parts (milled/turned/molded/stamped/printed) + dense scans | University lab / independent job shops | 4-8 weeks | $10,000 - $35,000 `[unverified]` |
| Shop-floor inference | 100-face B-rep part, local CPU ONNX | 8-core x86_64 or arm64 CPU, <= 400 MB RAM | 30-150 ms/part (`<= 250 ms` target) | $0 / month (local-first) |
| On-prem machine LoRA fine-tuning | 200-500 real shop parts (masked loss) | Single workstation GPU (RTX 4060/4070) or CPU | 15-30 min GPU / 2-3 hr CPU | $0 cloud (runs on-prem) |

## 7. Related documents

- `research/process-sampling-nn/PHYSICS-SURROGATES.md` — parametric surrogate physics-error equations for generating synthetic training fields.
- `research/process-sampling-nn/BENCHMARK-PROTOCOL.md` — real-world sim-to-real validation protocol, open benchmark artifacts and pass/fail tests for `H-1` through `H-4`.
- `decisions/D-049-process-aware-sampling-nn.md` — governance decision record.
- `spec/cmm/strat/proc/NODE.md` — `CMM.STRAT.PROC` specification node.
