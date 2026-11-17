---
id: SCAN.CLOUD.REG.GLOBAL
title: Global multi-scan registration command
level: 4
parent: SCAN.CLOUD.REG
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CLOUD.REG-001, REQ-SCAN.CLOUD.REG-002, REQ-SCAN.CLOUD.REG-004, REQ-SCAN.CLOUD.REG-005, REQ-SCAN.CLOUD.REG-006, REQ-SCAN.CLOUD.REG-007, REQ-SCAN.CLOUD.REG-008]
covers: [PW.PCL.05]
depends_on: [SCAN.CLOUD.REG.CTGT, SCAN.CLOUD.REG.GRAPH, SCAN.CLOUD.REG.RREP, ALN.BFIT.ICP.METRIC, ALN.BFIT.STOP, GEO.DIST.BATCH, GEO.FRAME.XFORM, KRN.NUM.LA.QR]
side_effect: W2
determinism: D0
cli: [fid cloud register-global]
mcp: [cloud_register_global]
journal: [cloud.register_global@1]
updated: 2026-11-17
---

# SCAN.CLOUD.REG.GLOBAL: Global multi-scan registration command

## Purpose

Registers N ≥ 3 overlapping scans simultaneously so that pairwise errors are spread over all overlaps instead of accumulating along a chain. The method is simultaneous multi-view point-to-plane ICP: each outer iteration builds point-to-plane rows for every edge of the pair graph in both directions, with Jacobians for both poses, and solves one joint linear least-squares step by Householder QR. It reuses ALN's row definitions and stopping rules, and its own state is only the joint system.

## Scope

**In:**
- `cloud.register_global@1`: N scan clouds with current poses, gauge (`fixed_scan` index or `group_frame`), overlap distance, minimum overlap, sample cap per pair direction, maximum correspondence distance, stop criteria
- Pair graph check before solving
- Joint step over 6(N − 1) (fixed scan) or 6N with the mean-twist gauge (group frame)
- Report with per-scan transforms, per-pair residuals and global RMS

**Out** (and which node owns it instead):
- Row and Jacobian formulas for one pose (owned by `ALN.BFIT.ICP.METRIC`); stopping rules (owned by `ALN.BFIT.STOP`)
- Pair graph and overlap (owned by `SCAN.CLOUD.REG.GRAPH`, `SCAN.CLOUD.REG.CTGT`)
- Report and version writing (owned by `SCAN.CLOUD.REG.RREP`)

## Requirements

- **REQ-SCAN.CLOUD.REG.GLOBAL-001** When `cloud.register_global@1` runs on N ≥ 3 scans, the system shall minimise the weighted sum of squared point-to-plane residuals over both directions of every pair-graph edge, with exactly one scan or the group frame held fixed as stated.
- **REQ-SCAN.CLOUD.REG.GLOBAL-002** Each outer iteration shall sample at most M moving points per edge direction by stride sampling, find correspondences with `OrientedCloudTarget` at the current poses within the maximum correspondence distance, and solve the joint linearised step by Householder QR (KRN.NUM), never by normal equations.
- **REQ-SCAN.CLOUD.REG.GLOBAL-003** With gauge `group_frame`, the joint step shall be constrained so that the sum of the N pose increments is zero, and the result shall record which gauge was used.
- **REQ-SCAN.CLOUD.REG.GLOBAL-004** If the pair graph at the start poses is disconnected, then the command shall fail with `scan.cloud.reg_disconnected` listing the components and shall create no element.
- **REQ-SCAN.CLOUD.REG.GLOBAL-005** The command shall stop by `ALN.BFIT.STOP` criteria applied to the largest translation and rotation increment over all scans, and shall record the algorithm ID `scan.reg.global_p2plane@1.0.0`, status and stop reason.
- **REQ-SCAN.CLOUD.REG.GLOBAL-006** If fewer than 6 rows per free scan remain in any iteration, then the command shall fail with `insufficient_correspondences` naming the scan.
- **REQ-SCAN.CLOUD.REG.GLOBAL-007** On fixture `ring_8_scans` (8 scans around a part, start errors up to 1 mm and 1°, σ = 10 µm), every recovered pose relative to the fixed scan shall lie within 10 µm and 5e-5 rad of truth, and the largest loop-closure error shall be at most half that of chained pairwise registration [unverified].
- **REQ-SCAN.CLOUD.REG.GLOBAL-008** On success, the command shall write registered versions of all N scans and a report through `SCAN.CLOUD.REG.RREP`, and the command shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/reg/ring_8_scans`, when `fid cloud register-global --scans <h0..h7> --fixed-scan 0 --json` runs, then REQ-007 holds and 8 registered versions with one shared group ID are created.
- **AC-2** Given the same data and `--gauge group_frame`, the poses differ from AC-1 by one common rigid transform (within 1e-9 m) and the mean increment is zero within 1e-12.
- **AC-3** (error path) Given `four_scans_two_islands`, the command fails with `scan.cloud.reg_disconnected` listing [[0,1],[2,3]] and journals nothing.
- **AC-4** Given 1 and 16 threads, outputs are bit-identical.
- **AC-5** (replay) Given the journal of AC-1, replay in exact mode gives `IDENTICAL`.

## Design

- **Registry command:** `cloud.register_global@1`, W2, D0. Params: `scans` [handles] (N ≥ 3, poses = current coordinates in the shared frame), `gauge` (`fixed_scan` with `fixed_scan` index default 0 | `group_frame`), `overlap_distance` m, `min_overlap` (default 0.1), `max_samples_per_edge` (default 20 000), `max_corr_distance` m, `stop` (`StopCriteria`), `accept_not_converged` (default false).
- **Algorithm:** graph = `pair_graph` at start poses; for k = 1..: for each edge (i, j) ascending and direction (i→j, j→i): sample source points, move by T_src, project onto target T_tgt via the cloud target (points mapped into the target's local coordinates by T_tgt⁻¹); world point w, world foot q, world normal n; row r = n·(w − q) with Jacobian blocks [(w × n)ᵀ, nᵀ] for the source scan and −[(q × n)ᵀ, nᵀ] for the target scan (ALN.BFIT.ICP.METRIC convention, order tx, ty, tz, a, b, c); drop the fixed scan's columns, or append six gauge rows Σξᵢ = 0 enforced by null-space elimination; Householder QR solve; compose increments with each pose (GEO.FRAME.XFORM); feed the largest increment and the objective to `StopMonitor`. Final statistics per edge and globally from the last correspondences.
- **Errors:** `scan.cloud.reg_disconnected`, `insufficient_correspondences`, `nan_in_iteration`, `scan.cloud.invalid_parameter` (N < 3).
- **Determinism:** D0 (fixed sampling, ascending edge order, fixed-order row assembly, deterministic QR).
- **Performance budget:** 20 scans, 40 edges, 20 000 samples per direction, 30 iterations under 60 s [unverified].
- **Crate:** `fid-scan-cloud`.

## Test fixtures

`fixtures/scan/reg/ring_8_scans` (seeded synthetic scans with known poses and a loop), `four_scans_two_islands`, CC0.

## Build prompt

> In the Rust crate `fid-scan-cloud`, register `cloud.register_global@1` (W2, D0; CLI `fid cloud register-global --scans <h>... [--fixed-scan I | --gauge group_frame] [--overlap-distance M] [--min-overlap F] [--max-samples-per-edge N] [--max-corr-distance M] [--max-iterations N] [--json]`, MCP `cloud_register_global`). Build the pair graph (`reg::graph::pair_graph`); disconnected fails `scan.cloud.reg_disconnected` with nothing created. Iterate simultaneous multi-view point-to-plane ICP: for each edge in ascending order and both directions, stride-sample source points, correspond with `OrientedCloudTarget` of the target scan, and add rows r = n·(w − q) with Jacobian [(w×n)ᵀ, nᵀ] for the source and the negated target block. Remove the fixed scan's columns or impose Σξ = 0 for `group_frame`. Solve by Householder QR from `fid-num` (KRN.NUM.LA.QR), compose increments, and stop with `StopMonitor` (ALN.BFIT.STOP) on the largest per-scan increment. Fewer than 6 rows for a free scan fails `insufficient_correspondences`. On success build `RegistrationReport` (algorithm ID `scan.reg.global_p2plane@1.0.0`) and `write_registered` for all scans.
>
> Constraints: CONSTITUTION §3, §8 (no panic/unwrap/unsafe), KNP-1 (no FMA, fixed-order reductions, no std HashMap/HashSet, NaN is an error). Tests: AC-1..AC-5.

## Open questions

- Pose-graph optimisation is a possible later alternative method for very large N; it would be a further leaf with its own algorithm ID.

## Children

_Leaf: none._
