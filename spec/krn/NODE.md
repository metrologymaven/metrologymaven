---
id: KRN
title: Metrology kernel: numerics, fitting, construction
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-006, REQ-FID-007]
covers: [GOM.CON.*, PW.CON.*, PCD.CON.*, SA.REL.*]
depends_on: [GEO]
cli: [fid fit *, fid construct *]
mcp: [fit_*, construct_*]
journal: [fit.*, construct.*]
updated: 2026-11-16
---

# KRN: Metrology kernel

## Purpose

The deterministic CPU kernel that computes values of record:
- the `fid-num` numerics facade (KNP-1);
- geometric feature fitting for every primitive and method;
- construction operations;
- outlier handling;
- the WASM reference artifact that makes results bit-reproducible.

## Scope

**In:**
- `fid-num`: dense linear algebra, transcendentals, total-order min/max, seeded RNG
- KNP-1 lints and their transitive enforcement
- Fits for point, line, plane, circle, slot, rectangle, cylinder, cone, sphere, torus, paraboloid, ellipse and curves, by least squares, minimum zone (Chebyshev), minimum circumscribed, maximum inscribed and tangential (inner and outer)
- Constructions: intersect, project, offset, midpoint, symmetry, bisector
- Live relationships (point-to-object, object-to-object) as recomputable constructions
- Outlier filters
- The kernel manifest and WASM reference build

**Out:**
- Alignment solvers (ALN, which uses KRN's optimisation primitives)
- GD&T interpretation (GDT)
- Uncertainty propagation (UNC)
- Mesh processing (SCAN)

## Requirements

- **REQ-KRN-001** The kernel shall be pure Rust, f64, with no I/O and no `unsafe`, and shall obey KNP-1, enforced by lints over its whole dependency graph.
- **REQ-KRN-002** The kernel shall fit every primitive listed in Scope by least squares, minimum zone, minimum circumscribed and maximum inscribed (where defined) and by tangential methods.
- **REQ-KRN-003** Least-squares and minimum-zone fits shall pass the public PTB TraCIM and NIST reference datasets for each covered primitive, and the pass/fail status shall be published per release.
- **REQ-KRN-004** The kernel shall provide construction operations (intersection, projection, offset, midpoint, symmetry, bisector) whose results are dependency-graph elements that recompute when their inputs change.
- **REQ-KRN-005** If a fit is ill-conditioned or fails to converge, then the kernel shall return a typed error with diagnostics and never a silently degraded result.
- **REQ-KRN-006** The kernel shall build as a WASM reference artifact whose outputs are bit-identical across hosts on the frozen reference dataset release.
- **REQ-KRN-007** Each algorithm shall have an ID `<module>.<feature>.<method>@<semver>` and a spec card stating its method, references, determinism class and validation evidence.
- **REQ-KRN-008** Outlier filtering in fits (sigma-based and robust) shall be explicit, parameterised and journaled, and the excluded points shall be recorded.

## Interfaces

- **Offers:** `fit.*` and `construct.*` commands; `fid-num`; the `Fit` trait; algorithm spec cards; the kernel manifest digest.
- **Consumes:** GEO point types only.

## Decomposition hints

NUM (fid-num + KNP-1 lints), LSQ (least-squares fits), MZ (minimum-zone and Chebyshev), ENV (circumscribed, inscribed, tangential), CON (constructions and relationships), OUT (outlier filters), MAN (kernel manifest, WASM artifact, spec cards).

## Split rationale

The split follows the **dependency layers of the kernel**, then **association method**. Numerics (NUM) and solvers (SOLV) are the shared base. The three fit families (LSQ, MZ, ENV) differ in objective, solver and validation evidence, and each has its own certification route (TraCIM Gaussian, TraCIM Chebyshev, open reference suites only). Constructions (CONS) and point selection (OUT) work on fit inputs and outputs and are used by every method. Identity and evidence (MAN) is cross-cutting release machinery that owns no geometry. The `Fit` trait lives in LSQ because every other fit starts from a least-squares solution.

The parent's open question is answered by making the solver and its KNP-1 audit its own subsystem (SOLV), because ALN and GDT also consume it and auditing a vendored solver is separate, reviewable work.

Child phases follow the concept baseline (docs/concept/05 §2.2): NUM, SOLV, LSQ, MZ and MAN in P0; ENV and CON in P1a; OUT in P1b. These are the first phases of each child under the P0 parent, not phase moves.

Alternatives considered:
- **Split by primitive** (lines, circles, cylinders...). Rejected: each primitive would repeat the solver, certificate and validation work for every method.
- **One FIT subsystem for all methods.** Rejected: too large (XXL), and it mixes P0 work that must pass TraCIM with P1a envelope work that has no external reference.
- **Fold the manifest and WASM build into NUM.** Rejected: NUM is a library; MAN is build, identity and evidence machinery with different reviewers.

## Open questions

- Reference datasets and the TraCIM client are owned by UNC (RDL), while REQ-KRN-003 publication is owned by KRN.MAN. UNC depends on KRN, so MAN consumes the datasets at test time only, with no build dependency.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.NUM | fid-num numerics facade and KNP-1 enforcement | 001 005 | none |
| KRN.SOLV | Deterministic optimisation solvers and certificates | 001 005 | none |
| KRN.LSQ | Fit contract and least-squares (Gaussian) fits | 002 003 005 007 | GOM.CON.18, PW.CON.01/02/03/07, SA.REL.01 |
| KRN.MZ | Minimum-zone (Chebyshev) fits | 002 003 005 007 | GOM.CON.19, PW.CON.05 |
| KRN.ENV | Envelope, tangential and constrained fits | 002 005 007 | GOM.CON.20, PW.CON.05 |
| KRN.CONS | Constructions and live relationships | 004 005 007 | GOM.CON.01-17/22, PCD.CON.01/03, PW.CON.04, SA.REL.02/03 |
| KRN.OUT | Point selection and outlier policies for fits | 005 008 | GOM.CON.21/23, PCD.CON.02, PW.CON.06, SA.REL.04 |
| KRN.MAN | Kernel identity, WASM reference artifact and validation evidence | 001 003 006 007 | none |
