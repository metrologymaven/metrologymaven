---
id: CMM.STRAT.PROC
title: Process-conditioned sampling strategy proposals
level: 3
parent: CMM.STRAT
kind: module
status: decision
decision: D-049
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-010]
covers: []
depends_on: [GEO.CAD, GDT.CHAR, CMM.STRAT.CHECK, CMM.STRAT.FEAT, CMM.STRAT.SCAN, CMM.PROBE.MODEL, CMM.PROBE.SENSOR]
cli: [fid cmm propose-process-strategy, fid cmm inspect-risk-map]
mcp: [cmm_propose_process_strategy, cmm_inspect_risk_map]
journal: [cmm.propose_process_strategy@1]
updated: 2026-11-21
---

# CMM.STRAT.PROC: Process-conditioned sampling strategy proposals

## Purpose

Proposes draft, provenance-tagged CMM sampling strategies (`MeasurementStrategy` and `ScanStrategy`) conditioned on **how the part was manufactured**—combining nominal B-rep CAD geometry, GD&T tolerance widths, target CMM accuracy (`MPE_E`) and manufacturing process metadata through a compact, offline, Process-Conditioned Geometric Neural Network (PC-GNN). It sits strictly as a proposer in front of the deterministic strategy validator (`CMM.STRAT.CHECK`), collision/path planner (`CMM.COLL`, `CMM.PATH`) and human motion-envelope gate (`H1`), and never computes a value of record or official measurement uncertainty (CONSTITUTION C1). Full architecture, surrogate physics-error generators and benchmark protocol are documented in `research/process-sampling-nn/`.

## Scope

**In:**
- Typed manufacturing process taxonomy and conditioning schema (`turning`, `milling_3axis`, `milling_5axis`, `injection_molding`, `die_casting`, `sheet_stamping`, `lpbf_additive`, `grinding`, `edm`, plus material class, workholding kind, primary process axis and missing-parameter masks)
- B-rep face-adjacency graph and per-face UV-grid / edge-curve feature extraction from `GEO.CAD`
- Offline local CPU ONNX inference for the base PC-GNN (~15M-50M parameters), with optional local machine/cavity LoRA adapter loading (training/management of private factory adapters lives in `ENT`)
- Per-face outputs: spatial deviation-risk map (`mu`, `sigma^2`), 16-bin spatial-frequency / harmonic risk spectrum, and recommended strategy prior (discrete vs. continuous scan, hit count / density, filter class)
- Deterministic compilation of risk maps + harmonic priors + tolerance-to-accuracy ratio (`T / MPE_E`) into candidate `MeasurementStrategy` / `ScanStrategy` proposals validated by `CMM.STRAT.CHECK`
- Automatic conservative fallback to uniform/dense sampling whenever predictive variance (`sigma^2`) exceeds a configured out-of-distribution threshold
- Provenance tagging: model ID, SHA-256 weight digest, optional adapter digest, process metadata digest and `agent_authored: true`

**Out** (and which node owns it instead):
- Minimum-point and non-degeneracy checks (`CMM.STRAT.CHECK`)
- Discrete hit and scan target generation from finalized parameters (`CMM.STRAT.FEAT`, `CMM.STRAT.SCAN`)
- Probe accessibility, collision verification and path planning (`CMM.COLL`, `CMM.PATH`, `CMM.SIM`)
- Motion authorization and human `H1` gate (`CMM.EXEC`)
- Official post-measurement uncertainty budgets `U` and conformance decisions (`UNC`, `GDT`, `KRN`)
- On-premises private fine-tuning of exact-machine / mold-cavity adapters and multi-CMM fleet drift dashboards (`ENT`, commercial add-on per D-049)

## Requirements

- **REQ-CMM.STRAT.PROC-001** The system shall define a versioned, typed manufacturing-process metadata schema covering process family, material class, workholding kind, primary process axis and optional process parameters with explicit missing-value flags.
- **REQ-CMM.STRAT.PROC-002** When proposing a strategy for a nominal part, the system shall extract the B-rep face-adjacency graph, per-face UV grids and edge curves from `GEO.CAD`, and run pinned-weight local CPU inference with zero outbound network calls (CONSTITUTION C7).
- **REQ-CMM.STRAT.PROC-003** For each candidate face, the proposer shall output a spatial deviation-risk map (`mu`, `sigma^2`), a harmonic-risk spectrum and a draft strategy proposal whose hit spacing avoids aliasing the predicted dominant harmonic modes.
- **REQ-CMM.STRAT.PROC-004** When the target feature's tolerance width `T` and target CMM accuracy profile `MPE_E` are supplied, the compiler shall scale proposed sampling density with `MPE_E / T` so tight-tolerance features receive higher sampling effort or continuous scanning where supported by `CMM.PROBE.SENSOR`.
- **REQ-CMM.STRAT.PROC-005** If a face's predicted variance `sigma^2` exceeds the configured out-of-distribution threshold, or if no matching process model is installed, then the system shall deterministically fall back to conservative uniform/dense sampling and flag the fallback in the proposal brief.
- **REQ-CMM.STRAT.PROC-006** When returning a proposed strategy, the system shall validate it through `CMM.STRAT.CHECK`, emit it only in draft state tagged `agent_authored: true`, and record the model ID, SHA-256 weight digest, optional adapter digest and process-input digest.
- **REQ-CMM.STRAT.PROC-007** The proposer shall never write a value of record, modify a measurement result or compute an official measurement uncertainty budget `U` (CONSTITUTION C1).

## Research-risk hypotheses (D-036 rule 4, pending D-049)

When this node is expanded into leaves and implemented, acceptance shall be gated on measurable hypotheses against the real-part benchmark suite defined in `research/process-sampling-nn/BENCHMARK-PROTOCOL.md`:

- **H-1 (Accuracy at fixed hit budget):** Across >= 30 held-out real manufactured parts spanning >= 2 process families (starting with CNC milling and turning), PC-GNN process-weighted sampling achieves >= 30% lower mean form/GD&T underestimation error than uniform sampling at equal point count `N`, measured against dense tactile CMM or optical 3D ground-truth scans.
- **H-2 (Cycle-time reduction at matched accuracy):** On the same held-out real-part suite, PC-GNN sampling reaches within 5% of dense-scan form error using >= 35% fewer touch-trigger hits than uniform sampling.
- **H-3 (Uncertainty calibration):** On held-out real parts, >= 95% of true peak surface deviations fall within the model's predicted `2*sigma` spatial risk envelope, and out-of-distribution geometries trigger the deterministic conservative fallback with >= 90% recall.
- **H-4 (Shop-floor local latency):** Quantized/ONNX inference on an 8-core x86_64/arm64 CPU completes in <= 250 ms per 100-face B-rep part with <= 400 MB peak resident memory and zero network I/O.

## Interfaces

- **Offers:** commands `cmm.propose_process_strategy@1` (R1 compute / W2 draft apply), `cmm.inspect_risk_map@1` (R1); data types `ManufacturingProcessSpec`, `FaceRiskMap`, `HarmonicRiskSpectrum`, `ProcessStrategyProposal`.
- **Consumes:** `GEO.CAD` B-rep topology and UV evaluation; `GDT.CHAR` tolerance widths; `CMM.PROBE.MODEL` and `CMM.PROBE.SENSOR` machine accuracy and scanning capability; `CMM.STRAT.CHECK`, `CMM.STRAT.FEAT`, `CMM.STRAT.SCAN` for compilation and validation.
- **Journal:** `cmm.propose_process_strategy@1` (records input digests, model/adapter digests and resulting draft strategy parameters).

## Decomposition hints

SCHEMA (process taxonomy and conditioning vector), GRAPH (B-rep face-adjacency and UV-grid extractor), INFER (offline ONNX runtime, weight digest check and adapter loader), COMP (risk-map + harmonic + MPE_E/T compiler with conservative OOD fallback and `CMM.STRAT.CHECK` gate), BENCH (synthetic-to-real evaluation harness for H-1..H-4).

## Open questions

- Blocked on **D-049** (founder acceptance of the PC-GNN boundary, open/ENT split and D-036 rule 4 amendment).
- Confirm exact ONNX runtime crate choice (`ort` vs. pure-Rust `tract`/`burn`) against `deny.toml` license allowlist and offline build rules before leaf build prompts are written [unverified].
- Determine whether `CMM.STRAT.PROC` runs in `D1` (per-platform bit-exact CPU ONNX) or `D2` determinism class when compiling strategies [unverified].

## Children

_None yet: status `decision` (blocked on D-049)._
