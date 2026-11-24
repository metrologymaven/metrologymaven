---
id: D-054
title: Dimensional metrology scope boundary, post-1.0 core expansions and Executable Metrology Field Guide
status: accepted
raised_by: founder
amends: [D-004, D-039]
blocks: []
decide_by: 2026-11-22
updated: 2026-11-22
---

# D-054: Dimensional metrology scope boundary, post-1.0 core expansions and Executable Metrology Field Guide

**Status: ACCEPTED 2026-11-22, autonomous under D-052.** Records the platform boundary across the full discipline of dimensional metrology and establishes the Executable Metrology Field Guide inside `OUTR.DOCS` and `OUTR.SITE`.

## Question

1. Should Metrology Maven expand its core workspace to cover *all* of dimensional metrology (from sub-nanometre optical/semiconductor metrology to industrial X-ray CT, 2D vision, on-machine CNC probing, calibration labs, and tolerance stack-up), or hold a defined 1.0 core with prioritized post-1.0 core tracks and an `ECO.SDK` plugin boundary?
2. Should the project publish a broad educational wiki covering all of dimensional metrology, or an executable, WASM-interactive Metrology Field Guide tied strictly to implemented kernel capabilities?

## Context

Metrology Maven's existing 26 domains (~2,400 projected leaves; ~54% shared horizontal platform infrastructure and ~46% macro-coordinate discipline code) target four focused apps (`Report`, `Inspect`, `Probe`, `Network`) built on a pure-Rust, CPU-only, `f64`, KNP-1 kernel (`C1`/`C2`) and a QIF 3.0 native model (`CONSTITUTION §5`).

Extending the core monorepo indiscriminately to every branch of dimensional metrology would roughly double leaf count (~4,600–5,600 leaves, or ~6,300+ with adjacent dimensional engineering) while multiplying validation, hardware-SDK, and architectural effort by 2.5×–4×—primarily because multi-gigabyte CT voxel volumes, tomographic reconstruction, live vision image pipelines, proprietary NDA-gated controllers, and optical/nano physics strain `C1`/`C2` CPU-only `f64` execution and the QIF-native data model.

At the same time, three missing industrial tracks align directly with the existing kernel, `HAL`, `UNC`, and `RPT` architecture and already have partial hooks (`HAL.IDL.SENS.SURF`, `HAL.SIM.SURFCAM`, `UNC.ORS.FLT`, `UNC.INST`, `RPT.STATS`).

On education, publishing a broad prose-only wiki covering unimplemented disciplines before 1.0 risks violating Honest Status (`CONSTITUTION §8`), dilutes engineering focus, and risks subtle unreviewed standards errors outside the project's automated mathematical oracles. By contrast, an interactive field guide powered directly by the project's own Rust-to-WASM kernel crates (`fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`) delivers immediate pre-1.0 utility, recruits reviewers (`OUTR.GDTC`), supports teaching (`OUTR.PROG`), and doubles as offline-bundled in-app reference documentation.

## Decision

1. **Preserve the 1.0 four-app macro-coordinate core (`D-004`, `launch-1` unchanged).**
   - `Report` and `Inspect` remain the `launch-1` beachhead (`P0`–`P1b`), followed by `Probe` (`P2`) and `Network` (`P3`).

2. **Adopt three high-ROI post-1.0 core expansion tracks in the spec tree:**
   - **MSA & non-normal capability (`RPT.STATS.MSA`, starts `P3`):** AIAG MSA 4th Ed. (Type 1, crossed/nested ANOVA Gage R&R, linearity/bias, stability, attribute agreement), VDA 5 (`Q_MS`/`Q_MP`) linked to `UNC`, and ISO 22514-2 non-normal capability distributions (Weibull, Rayleigh, folded-normal, Box-Cox). Excluded from `launch-1`.
   - **Surface texture & precision form (`SURF`, L1 domain, starts `P3`):** Completes `CONSTITUTION §7`'s P3 "Form and texture" theme and connects to `HAL.IDL.SENS.SURF`, `HAL.SIM.SURFCAM`, and `UNC.ORS.FLT`: ISO 16610 profile and areal filtration (`SURF.FILT`), ISO 21920 / ASME B46.1 2D profile texture (`SURF.PROF`), ISO 25178 3D areal texture (`SURF.AREAL`), ISO 12181/12180 harmonic roundness and spindle separation (`SURF.FORM`), 2D stylus contour tracing (`SURF.CONT`), and X3P (`ISO 25178-72`) / softgauge datasets (`SURF.IO`).
   - **Calibration-lab & machine verification (`CAL`, L1 domain, starts `P4`):** ISO 10360 CMM/arm/tracker/optical acceptance and reverification (`CAL.CMM`), ISO 230 machine-tool interferometry/error-mapping/ballbar (`CAL.MACH`), length/angle/gauge-block/thread-gauge/surface-plate/hand-tool calibration (`CAL.GAUGE`), ISO/IEC 17025 calibration certificates, CMC/TUR guardbanding and PTB DCC XML (`CAL.CERT`), and calibration asset/interval lineage (`CAL.ASSET`).

3. **Hold a clear plugin and companion-project boundary (`ECO.SDK`, `IND.CONTRACT`) for architecture-stretching branches:**
   - Industrial X-ray CT (`GOM.CT.*`, analysis and tomographic reconstruction), 2D/2.5D Vision Metrology (`VMM`), On-Machine CNC Probing (`OMV`) and closed-loop CNC offsets, Micro/Nano/Semiconductor/Optical Interferometry (`ISO 10110`, AFM/SPM, CD-SEM, OCD, thin film), extended mechanical packs (threads, bevel/worm gears, splines, cams/cranks, welds), photogrammetry image engines, and adjacent dimensional engineering (3D tolerance stack-up, reverse engineering `PW.MOD.*`, optical DIC `GOM.COR.*`) remain outside the core workspace as `ECO.SDK` plugins or companion projects. Re-admitting any of them to core requires a dedicated decision record addressing `C1`/`C2`, data-model extensions, and CI budgets.

4. **Build an Executable, WASM-Interactive Metrology Field Guide (`OUTR.DOCS.GUIDE`, starts `P0`) rather than a detached prose-only wiki:**
   - Lives under `OUTR.DOCS.GUIDE` and publishes at `/guide/` on `metrologymaven.io` (`OUTR.SITE`).
   - Grows strictly in lockstep with already-implemented kernel crates—starting with P0/P1a feature fits (`fid-fit`), GD&T tolerance zones and DRFs (`fid-gps`), GUM/Monte Carlo uncertainty (`fid-kunc`), ISO 14253 / JCGM 106 decision rules and risk (`fid-kdec`), and KNP-1 / RFC 8785 cryptographic replay (`fid-canon`).
   - Embeds live browser sandboxes powered by a safe-Rust (`#[forbid(unsafe_code)]`) `wasm32-wasip1` wrapper (`crates/fid-guide-wasm`, `OUTR.DOCS.GUIDE.WASM`) executed through a zero-dependency, same-origin WASI loader with zero third-party origins (`REQ-OUTR.SITE-007`), graceful no-JS static fallbacks, and CSP `script-src 'self' 'wasm-unsafe-eval'` scoped exclusively to `/guide/` (all other site routes keep `script-src 'none'`).
   - Carries one concise **Dimensional Metrology Field Map** (`OUTR.DOCS.GUIDE.MAP`) mapping all 10 major pillars (0.1 nm to 100 m) directly to live spec-tree counts and this decision's scope boundary.
   - Obeys `AGENTS.md` rule 5 (standards cited by designator, edition, and clause only—never pasted standards text), `D-045` (`interpretation: provisional` visibly shown on unreviewed rules), and `OUTR.BRAND` vocabulary rules.

## Consequences

- `spec/CONSTITUTION.md` §2.1 and §7 include `SURF` (`P3`) and `CAL` (`P4`) in the L2 domain service order and phase table.
- `spec/NODE.md` adds `REQ-FID-032` (`SURF`), `REQ-FID-033` (`CAL`), and `REQ-FID-034` (`OUTR.DOCS.GUIDE`), with `SURF` and `CAL` added as L1 domains split into open L2 subsystems.
- `spec/rpt/stats/NODE.md` adds `REQ-RPT.STATS-015` satisfied by `RPT.STATS.MSA` (`P3`, open), which is explicitly excluded from `milestones/launch-1.md`.
- `spec/outr/docs/NODE.md` adds `REQ-OUTR.DOCS-011` satisfied by `OUTR.DOCS.GUIDE` (`P0`, split into four build-ready leaves: `WASM`, `MAP`, `TOPICS`, `SITE`).
- `launch-1` remains unchanged in product scope while gaining the P0 Executable Metrology Field Guide immediately.
