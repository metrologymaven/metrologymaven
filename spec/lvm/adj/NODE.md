---
id: LVM.ADJ
title: Network and bundle adjustment
level: 2
parent: LVM
kind: subsystem
status: split
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-LVM-002, REQ-LVM-007]
covers: [SA.NET.01, SA.NET.03]
depends_on: [LVM.STATION, UNC, KRN, GEO]
cli: [fid net run-adjustment, fid net set-datum, fid net *-weighting]
mcp: [net_run_adjustment, net_set_datum, net_*_weighting]
journal: [net.run_adjustment@1, net.set_datum@1, net.*_weighting@1]
updated: 2026-11-16
---

# LVM.ADJ: Network and bundle adjustment

## Purpose

The solver that turns the observation store into one consistent network: adjusted station poses, adjusted common-point coordinates, residuals and a full covariance. Every observation is weighted from its instrument's UNC model, so a tracker distance, an arm point and a total-station angle each count according to its stated uncertainty. The adjustment runs on the deterministic kernel and works identically on live, recorded or imported data.

## Scope

**In:**
- Weighted least-squares adjustment of all stations and common points (unified network)
- Observation equations for polar, direction-only and Cartesian-with-covariance observations, and for scale constraints supplied by `LVM.SCALE`
- Weighting from UNC instrument models, with explicit user-declared weights as a labelled fallback
- Datum definition: fixed station, fixed points, minimal constraints or inner constraints
- Initial values from station-move solves or ALN point-pair transforms
- Convergence control, rank-deficiency detection
- A-posteriori statistics: residuals, standardised residuals, variance factor, redundancy numbers, adjusted covariance
- Gross-error flagging of observations
- Offline adjustment with no instrument connected

**Out** (and which node owns it instead):
- Observation storage and import (`LVM.STATION`)
- Scale-bar definitions and their observation form (`LVM.SCALE`)
- Station-move quick solves and geometry pre-checks (`LVM.MOVE`)
- Presentation and export of residual reports (`LVM.RPT`)
- Instrument uncertainty models themselves (`UNC`)
- Single-instrument alignments (`ALN`)

## Requirements

- **REQ-LVM.ADJ-001** The system shall adjust all enabled stations and common points in one weighted least-squares solution and return adjusted station poses, adjusted point coordinates and their full covariance matrix.
- **REQ-LVM.ADJ-002** The system shall weight each observation from the UNC instrument model referenced by its station, and shall record the model ID and version in the adjustment result.
- **REQ-LVM.ADJ-003** If a station has no UNC instrument model, then the adjustment shall refuse to run unless the user declares explicit weights for that station, and the result shall label those weights as user-declared.
- **REQ-LVM.ADJ-004** If the network datum is undefined or the normal matrix is rank-deficient, then the adjustment shall stop with an error naming the undetermined parameters and shall journal no result.
- **REQ-LVM.ADJ-005** Given a noise-free synthetic network of at least 6 stations and 100 common points, the adjusted coordinates shall match the generating truth within 1e-9 m [unverified].
- **REQ-LVM.ADJ-006** The adjustment shall report residuals and standardised residuals per observation, the a-posteriori variance factor and redundancy numbers, and shall flag observations whose standardised residual exceeds a user-set threshold (default 3) [unverified].
- **REQ-LVM.ADJ-007** The adjustment shall produce the same result from recorded or imported observations as from the same observations captured live, and shall need no connected instrument.
- **REQ-LVM.ADJ-008** A network of 50 stations and 5000 common points shall adjust in under 10 s on reference hardware [unverified].
- **REQ-LVM.ADJ-009** Adjusted uncertainties shall be attached to downstream ResultRecords with a UEC class from UNC, and shall never be reported when the variance factor test was not evaluated.

## Interfaces

- **Offers:** `net.run_adjustment@1` (W2, determinism D3 iterative), `net.set_datum@1` (W2), weighting commands (W2); adjusted network frames for ALN and GDT; covariance for UNC propagation.
- **Consumes:** `LVM.STATION` observations; `LVM.SCALE` scale constraints; UNC instrument models; KRN `fid-num` linear algebra and optimisation primitives; GEO frames.

## Decomposition hints

OBSEQ (observation equations per type), WEIGHT (UNC-driven weighting and fallback), DATUM (datum definition and rank checks), SOLVE (iteration, convergence, covariance), STATS (residual statistics and gross-error flags).

## Split rationale

The dividing principle is **the stages of one least-squares solve**: what each observation predicts (OBSEQ), how much it counts (WEIGHT), what fixes the frame and where iteration starts (DATUM), the solve itself (SOLVE) and what the solution says about its own quality (STATS). Image-level photogrammetry (IMG) is a separate, later extension because it adds new parameters (cameras) and a new observation type rather than changing the stages.

Alternatives considered:
- **Split by observation type** (polar, Cartesian, image). Rejected: weighting, datum and statistics are shared by all types; only OBSEQ and IMG are type-specific.
- **Merge STATS into SOLVE.** Rejected: statistics have their own open reviewer questions (gross-error test, scaling) and UEC gating, and must not hold up the solver.

## Open questions

- **Owner assigned (level-2 review, accepted 2026-11-16):** LVM.ADJ owns image-level photogrammetry bundle adjustment as a later extension (phase `later` until decided otherwise). Placed in `LVM.ADJ.IMG`.
- Which gross-error test (data snooping, tau test or another) and which thresholds should be the default? Needs a geodesy or large-volume metrology reviewer. Carried in `LVM.ADJ.STATS`.
- Should robust estimators (for example iteratively reweighted) be offered, or only flagging with manual exclusion? Robust reweighting changes values of record and needs human adoption under C5. Carried in `LVM.ADJ.STATS`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| LVM.ADJ.OBSEQ | Observation equations and Jacobians | 001 | none |
| LVM.ADJ.WEIGHT | Observation weighting from UNC instrument models | 002 003 | SA.NET.03 |
| LVM.ADJ.DATUM | Datum definition, initial values and rank checks | 004 | none |
| LVM.ADJ.SOLVE | Adjustment solve, convergence and covariance | 001 005 007 008 | SA.NET.01 |
| LVM.ADJ.STATS | Residual statistics, gross-error flags and UEC gating | 006 009 | none |
| LVM.ADJ.IMG | Image-level photogrammetry bundle extension (later) | 001 002 | none |
