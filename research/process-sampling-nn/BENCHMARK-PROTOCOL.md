# Sim-to-Real Validation Benchmark Protocol

Companion to `research/process-sampling-nn/DESIGN.md` and `spec/cmm/strat/proc/NODE.md` (D-049, D-036 rule 4).

## 1. Why real physical parts are mandatory

A neural network trained on synthetic physics surrogates and evaluated only on those same surrogates proves nothing about shop-floor performance. Real parts contain unmodeled clamping distortion, tool wear, residual stress, heat-treat warpage, surface roughness and thermal gradients.

This protocol replaces subjective expert opinion with **objective, reproducible measurement on real physical parts**—collected independently through small/midsize job shops and university metrology labs using open formats (STEP AP242, QIF 3.0, PLY/STL), with zero proprietary vendor software or NDA-locked OEM designs.

## 2. Open-source benchmark artifact family ("torture-test" parts)

Design and publish 5 CC-BY-4.0 / open-CAD benchmark parts (STEP AP242 with semantic PMI) that remain completely non-ITAR and publicly shareable:

| Artifact ID | Primary process | Deliberate error-inducing geometry | Key GD&T callouts |
|---|---|---|---|
| `MM-BENCH-01` | 3/5-axis CNC milling (6061-Al, 304-SS) | Thin vertical wall (1.5 mm), deep narrow pocket (`L/D > 4`), tight internal corners, datum boss | Flatness, profile of a surface, perpendicularity, cylindricity |
| `MM-BENCH-02` | 3-jaw CNC turning (4140 steel, 6061-Al) | Thin-walled bore + stepped OD + cantilevered shaft end | Circularity (roundness), cylindricity, coaxiality, runout |
| `MM-BENCH-03` | Injection molding / 3D-printed mold proxy (ABS, nylon) | Ribbed housing flange with varying wall thickness, bosses and parting line | Flatness, surface profile, position of boss pattern |
| `MM-BENCH-04` | Sheet metal stamping / press brake (1.5 mm steel/Al) | Multi-bend bracket with holes placed near bend radii and long free flanges | Profile, angularity, hole position relative to datum tabs |
| `MM-BENCH-05` | Metal additive LPBF (316L / Ti-6Al-4V or AlSi10Mg) | Overhanging arch, thin unsupported fins, vertical and horizontal bores | Surface profile, flatness, cylindricity |

### Minimum dataset size for v1 gate (`H-1`..`H-4`)
- **Phase I / narrow proof (milling + turning):** >= 30 physical parts (>= 15 milled, >= 15 turned) across >= 3 distinct machines/shops.
- **Full v1 multi-process benchmark:** >= 100 physical parts across all 5 process families and >= 5 independent facilities.

## 3. Ground-truth acquisition protocol

Each physical benchmark part is measured to obtain a **dense ground-truth surface deviation map** registered to its nominal CAD (via deterministic `ALN` / `KRN` alignment):

- **Preferred ground truth:** Continuous tactile scanning CMM (e.g., analog scanning head, point spacing <= 0.2 mm) for high-accuracy machined/turned features (micrometer accuracy).
- **Complementary ground truth:** Calibrated structured-light 3D scanner (calibrated per VDI/VDE 2634 or ISO 10360-8, point spacing <= 0.15 mm) for full-field molded, stamped and additive parts where dense tactile scanning is prohibitively slow.
- **Recorded metadata (QIF 3.0 + JSON manifest):** nominal STEP digest, process family, material batch, machine class (anonymized ID), workholding type, ambient temperature, instrument calibration accuracy (`MPE_E`), and raw dense point cloud / scan trace.

> **Why dense scans for *benchmarking* while runtime uses normal CMMs:** Touch-trigger CMM programs sample only 4-10 points per feature and cannot reveal the true peak deviation *between* hits. Dense scans provide the oracle surface needed to score how accurately a sparse probing plan estimates true form/GD&T error. Once trained and validated, runtime inference requires **CAD + process metadata only**—zero scans on the shop floor.

## 4. Virtual probing evaluation & baselines

For every held-out real part and every toleranced feature `f`:
1. Compute the **ground-truth characteristic value `V_true(f)`** on the dense ground-truth scan using the deterministic `KRN` / `GDT` evaluator (minimum-zone / Chebyshev or standard-mandated fit).
2. Given a hit budget `N` (evaluated across `N in {6, 9, 12, 18, 25, 40}`), generate hit target locations on the nominal CAD face using three methods:
   - **Baseline 1 (Uniform / blind):** equi-parametric / stratified uniform grid (or equal-angle circle rings).
   - **Baseline 2 (Geometry / curvature-only):** curvature-weighted NURBS/B-rep sampling (no process metadata).
   - **Candidate (PC-GNN):** `CMM.STRAT.PROC` process-weighted sampling + anti-aliasing harmonic spacing + `CMM.STRAT.CHECK` validation.
3. Sample the real dense scan at each method's target hit locations (with realistic probe tip radius compensation and Gaussian probing noise matching the CMM's repeatability) and compute `V_est(f)` with the identical deterministic `KRN` / `GDT` evaluator.
4. Compute **underestimation error** `e(f, N) = max(0, V_true(f) - V_est(f)) / max(V_true(f), eps)` (in metrology, underestimating a form/GD&T deviation causes false acceptance of out-of-tolerance parts).

## 5. Pass/fail acceptance gates (D-036 rule 4)

All tests run on **strictly held-out real parts** never seen during training or hyperparameter tuning:

| Hypothesis | Metric | Pass threshold | Statistical check |
|---|---|---|---|
| **H-1 (Accuracy at fixed N)** | Mean relative form/GD&T underestimation error vs. Baseline 1 (Uniform) at matched `N` | >= 30% reduction across held-out parts (`>= 2` process families) | Paired bootstrap 95% CI strictly above 30% |
| **H-2 (Hit reduction at matched accuracy)** | Minimum `N` to reach `e(f, N) <= 0.05` vs. Baseline 1 (Uniform) | >= 35% fewer touch-trigger hits | Paired Wilcoxon signed-rank test (`p < 0.01`) |
| **H-3 (Calibration & OOD safety)** | Coverage of true peak deviation inside predicted `mu + 2*sigma` envelope; OOD fallback recall on unseen process/geometry | >= 95% peak coverage; >= 90% OOD fallback recall | Clopper-Pearson binomial lower bound |
| **H-4 (Local CPU latency)** | End-to-end graph extraction + ONNX inference + strategy compilation on 100-face B-rep | `<= 250 ms` p95 wall time, `<= 400 MB` RSS, 0 network packets | Automated benchmark in CI / release runner |

## 6. Shop-local machine adapter benchmark (Layer 2 / `ENT`)

When evaluating the optional on-premises Layer 2 LoRA adapter on a single machine's production lot (`>= 50` sequential parts):
- Split chronologically: first 25 parts for local on-prem LoRA fine-tuning (using masked loss at actual probed points), next 25 parts held out.
- **Target:** Layer 2 adapter reduces form-error underestimation by a further **>= 15%** relative to the Layer 1 base process model on that specific machine `[unverified target]`, while raising an anomaly flag within `<= 3` parts when an artificial systematic error (simulated tool wear / fixture shift) is injected.
