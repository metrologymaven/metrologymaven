---
id: CMM.STRAT
title: Feature and scan measurement strategies
level: 2
parent: CMM
kind: subsystem
status: split
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM-002]
covers: [PCD.FEA.*, PCD.SCN.*]
depends_on: [KRN, GEO, GDT, CMM.PROBE]
cli: [fid cmm *-strategy*, fid cmm *-feature*, fid cmm *-scan*]
mcp: [cmm_*_strategy*, cmm_*_feature*, cmm_*_scan*]
journal: [cmm.*_strategy*@*, cmm.*_feature*@*, cmm.*_scan*@*]
updated: 2026-11-21
---

# CMM.STRAT: Feature and scan measurement strategies

## Purpose

Decides **where and how to touch** a nominal feature: the number and layout of hits, the depth and levels, the scan type, density and speed class, and which tip it uses. A strategy is a declarative, machine-independent description that the path planner later turns into motion. It exists as a separate unit because strategies are what metrology engineers author and reuse; they encode the fit method's point needs and the sampling policy, independent of the machine and fixture.

## Scope

**In:**
- Auto-feature strategy types: point, vector and surface points, edge point, line, plane, circle, cylinder, cone, sphere, slot
- Sampling parameters: hit count, levels, depth, start and end angles, spacing, sample hits for local surface normal correction
- Scan strategies: linear open and closed, patch and UV, perimeter, high-speed analog (continuous contact) scans, with density and speed class
- Scan filtering settings applied to acquired scan data (filter type and parameters as part of the strategy; filter algorithms come from `KRN`)
- Manually learned features: turning observed manual hits into a nominal feature plus a reusable strategy
- Feature sets: grouping features to be measured as one set with shared strategy settings
- Validation of each strategy against the minimum-point and distribution needs of its fit method
- Strategy defaults and named strategy libraries
- Optional process-aware strategy proposals conditioned on CAD geometry, tolerances and manufacturing process metadata (pending D-049)

**Out** (and which node owns it instead):
- Moves between features, clearance and approach motion (owned by `CMM.PATH`)
- Choosing features from CAD or PMI and assembling programs (owned by `CMM.GEN`)
- Probe and sensor definitions (owned by `CMM.PROBE`)
- Fitting and outlier filter algorithms (owned by `KRN`)
- Evaluation of characteristics (owned by `GDT`)
- Reachability and collision verdicts (owned by `CMM.COLL`)

## Requirements

- **REQ-CMM.STRAT-001** The system shall provide a strategy type for each of point, vector point, surface point, edge point, line, plane, circle, cylinder, cone, sphere and slot features.
- **REQ-CMM.STRAT-002** The system shall express every strategy as hit or scan targets (position and approach vector, SI metres) in the feature's nominal frame, generated deterministically (class D0) from the nominal feature and the strategy parameters.
- **REQ-CMM.STRAT-003** If a strategy yields fewer points than the minimum its fit method requires, or a degenerate distribution (for example collinear hits for a plane), then the system shall reject the strategy with a typed error naming the shortfall.
- **REQ-CMM.STRAT-004** The system shall support linear open, linear closed, patch (UV), perimeter and continuous analog scan strategies, each with a point density or spacing parameter and a speed class.
- **REQ-CMM.STRAT-005** If a continuous analog scan strategy is assigned to a tip whose configuration is not a scanning probe, then the system shall reject the assignment.
- **REQ-CMM.STRAT-006** The system shall store scan filter settings as part of the scan strategy, and shall record the filter algorithm ID and version with every filtered scan result.
- **REQ-CMM.STRAT-007** When a user supplies manual hits for an unknown feature, the system shall propose a feature type and nominal from those hits and create a strategy that reproduces the hit layout, as a draft the user must accept.
- **REQ-CMM.STRAT-008** The system shall support feature sets whose members share strategy settings, and shall apply a change to the set's settings to every member in one journaled command.
- **REQ-CMM.STRAT-009** The system shall support named, versioned strategy libraries, and every feature shall record the library entry and version its strategy came from.
- **REQ-CMM.STRAT-010** When manufacturing process metadata and nominal CAD geometry are supplied, the system shall support proposing draft, provenance-tagged measurement strategies conditioned on predicted process-induced deviation risk, validated through `CMM.STRAT.CHECK` and falling back to conservative uniform sampling when predictive uncertainty exceeds the configured bound (D-049).

## Interfaces

- **Offers:** commands such as `cmm.set_strategy@1`, `cmm.learn_feature@1`, `cmm.create_feature_set@1` (W2), `cmm.preview_strategy@1`, `cmm.propose_process_strategy@1` (R1); data types MeasurementStrategy, ScanStrategy, HitTarget, FeatureSet, ProcessRiskMap.
- **Consumes:** `KRN` fit method point requirements and filters; `GEO` nominal feature geometry and normals; `GDT` tolerance bands for strategy sizing; `CMM.PROBE` tip types and accuracy profiles.
- **Journal:** `cmm.*_strategy*`, `cmm.*_feature*`, `cmm.*_scan*`.

## Decomposition hints

FEAT (auto-feature strategies per type), SCAN (scan strategy types), FILT (scan filter settings), LEARN (manually learned features), SET (feature sets), LIB (strategy libraries and defaults), CHECK (minimum-point validation), PROC (process-conditioned strategy proposals, D-049).

## Open questions

- Edge point and slot strategies may need GDT input on which points define the measurand; confirm with `GDT` when that domain splits.
- Default sampling counts per feature type are a domain-expert choice; incumbents' defaults are not copied [unverified].
- Process-conditioned neural strategy proposals (`CMM.STRAT.PROC`) are blocked on D-049.

## Split rationale

The split separates **what a strategy is** from **the checks and groupings around it**. FEAT and SCAN own the two families of target generators (discrete hits and continuous scans), which differ in parameters, geometry and sensor needs. CHECK is a shared gate that both families and the program generators call, and it owns the HitTarget type so the dependency points one way. FILT is cut from SCAN because filter provenance is a result-record concern with its own small command. LEARN, SET and LIB are workflows on top of strategies (teach from hits, group, reuse) with their own commands and no shared state with each other. PROC is isolated as a P3 research-risk proposer (D-049) so optional ML inference and synthetic-to-real benchmarks never contaminate the deterministic core strategy generators or block P2 delivery.

Alternatives considered:
- **One child per feature type.** Rejected: eleven near-identical children that would duplicate the command surface and the check.
- **CHECK inside FEAT.** Rejected: scan strategies and `CMM.GEN` also need it.
- **Folding PROC into LIB.** Rejected: LIB stores static, versioned entries while PROC runs a learned spatial risk model with research-risk hypotheses and an optional ONNX runtime.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [CMM.STRAT.FEAT](feat/NODE.md) | Auto-feature strategy types and hit generation | 001 002 | PCD.FEA.01, PCD.FEA.02, PCD.FEA.05 |
| [CMM.STRAT.CHECK](check/NODE.md) | Minimum-point and distribution check for strategies | 003 | |
| [CMM.STRAT.SCAN](scan/NODE.md) | Scan strategy types | 004 005 | PCD.SCN.01, PCD.SCN.02, PCD.SCN.03, PCD.SCN.04 |
| [CMM.STRAT.FILT](filt/NODE.md) | Scan filter settings and filter provenance | 006 | PCD.SCN.05 |
| [CMM.STRAT.LEARN](learn/NODE.md) | Manually learned features | 007 | PCD.FEA.03 |
| [CMM.STRAT.SET](set/NODE.md) | Feature sets with shared strategy settings | 008 | PCD.FEA.04 |
| [CMM.STRAT.LIB](lib/NODE.md) | Strategy libraries and defaults | 009 | |
| [CMM.STRAT.PROC](proc/NODE.md) | Process-conditioned sampling strategy proposals (decision D-049) | 010 | |
