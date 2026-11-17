---
id: LVM.ADJ.SOLVE
title: Adjustment solve, convergence and covariance
level: 3
parent: LVM.ADJ
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.ADJ-001, REQ-LVM.ADJ-005, REQ-LVM.ADJ-007, REQ-LVM.ADJ-008]
covers: [SA.NET.01]
depends_on: [LVM.ADJ.OBSEQ, LVM.ADJ.WEIGHT, LVM.ADJ.DATUM, KRN.SOLV, CORE.JOB]
cli: [fid net run-adjustment]
mcp: [net_run_adjustment]
journal: [net.run_adjustment@1]
updated: 2026-11-16
---

# LVM.ADJ.SOLVE: Adjustment solve, convergence and covariance

## Purpose

Runs the unified network adjustment: assembles the weighted observation equations of every enabled observation and constraint, iterates to convergence, and commits adjusted station poses, adjusted point coordinates and their full covariance as one journaled result. It is the command `net.run_adjustment@1` and the place where the accuracy, offline-equivalence and performance requirements of `LVM.ADJ` are met.

## Scope

**In:**
- `net.run_adjustment@1` as a long-running, cancellable job
- Deterministic parameter ordering (stations then points, in creation order)
- Sparse normal-equation assembly with fixed-order reductions
- Gauss-Newton iteration with convergence and iteration limits
- Full covariance of adjusted parameters
- Committing adjusted poses into the station slots and the result record, with input digests
- Offline-equivalence: identical results from live, transcript or file observations

**Out** (and which node owns it instead):
- Observation equations (`LVM.ADJ.OBSEQ`), weights (`LVM.ADJ.WEIGHT`), datum and initial values (`LVM.ADJ.DATUM`)
- Residual statistics and gross-error flags (`LVM.ADJ.STATS`)
- Image-coordinate bundles (`LVM.ADJ.IMG`)
- Generic sparse solvers and certificates (`KRN.SOLV`)

## Requirements

- **REQ-LVM.ADJ.SOLVE-001** When `net.run_adjustment@1` runs, the system shall solve all enabled stations, enabled common points and enabled constraints in one weighted least-squares problem and commit adjusted poses, adjusted coordinates and the full parameter covariance as one journal event.
- **REQ-LVM.ADJ.SOLVE-002** The system shall stop iterating when the largest parameter update is below 1e-10 m for translations and coordinates and 1e-12 rad for rotations [unverified], and shall fail with `NET_NOT_CONVERGED` after a configurable iteration limit (default 50), journaling no result.
- **REQ-LVM.ADJ.SOLVE-003** Given fixture `fixtures/lvm/net-6st-100pt-noisefree`, the adjusted coordinates shall match the generating truth within 1e-9 m [unverified].
- **REQ-LVM.ADJ.SOLVE-004** Given the same observations from a live session and from its transcript or file import, the adjustment results shall be bit-identical on the same platform.
- **REQ-LVM.ADJ.SOLVE-005** A network of 50 stations and 5000 common points shall adjust in under 10 s on reference hardware [unverified].
- **REQ-LVM.ADJ.SOLVE-006** The adjustment result shall record the algorithm ID, kernel version, datum, weighting labels, UNC model IDs and versions, input digests and the `simulated` flag.
- **REQ-LVM.ADJ.SOLVE-007** When the job is cancelled, the system shall commit nothing and leave the station pose slots unchanged.

## Interfaces

- **Offers:** `net.run_adjustment@1` (W2, determinism D3); `AdjustmentResult` (poses, coordinates, covariance, metadata) for `LVM.ADJ.STATS`, `LVM.SCALE.CHECK`, `LVM.RPT`, ALN and GDT.
- **Consumes:** `LVM.ADJ.OBSEQ`, `LVM.ADJ.WEIGHT`, `LVM.ADJ.DATUM`; `KRN.SOLV` sparse factorisation; `CORE.JOB` progress and cancellation.

## Decomposition hints

ASSEMBLE (parameter ordering and sparse normal equations), ITER (Gauss-Newton loop and convergence), COV (covariance extraction), COMMIT (result record and pose-slot write).

## Open questions

- Should Levenberg-Marquardt damping be offered for poor initial values, or should failures go back to `LVM.ADJ.DATUM` initial values only?

## Children

_None yet: status `open`._
