---
id: SCAN.CLOUD.REG.PAIR
title: Pairwise scan registration command
level: 4
parent: SCAN.CLOUD.REG
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CLOUD.REG-001, REQ-SCAN.CLOUD.REG-002, REQ-SCAN.CLOUD.REG-003, REQ-SCAN.CLOUD.REG-005, REQ-SCAN.CLOUD.REG-007, REQ-SCAN.CLOUD.REG-008]
covers: [PW.PCL.05]
depends_on: [SCAN.CLOUD.REG.CTGT, SCAN.CLOUD.REG.RREP, ALN.BFIT.ICP.LOOP, ALN.BFIT.STOP, ALN.MODEL.ELEM, GEO.FRAME.XFORM]
side_effect: W2
determinism: D0
cli: [fid cloud register-pair]
mcp: [cloud_register_pair]
journal: [cloud.register_pair@1]
updated: 2026-11-17
---

# SCAN.CLOUD.REG.PAIR: Pairwise scan registration command

## Purpose

Registers one moving scan to one fixed scan by calling ALN's best-fit loop with the fixed scan as an oriented-cloud reference, then writes the registered version and a one-pair report. It defines no solver of its own: the iteration, metrics, locks and stopping rules are ALN's.

## Scope

**In:**
- `cloud.register_pair@1`: moving cloud, fixed cloud, initial pose (explicit rigid transform, an alignment element from `ALN.PRE`, or identity), metric, maximum correspondence distance, sample cap, overlap distance, stop criteria
- Call of `bestfit` with `OrientedCloudTarget` as reference
- Overlap fraction after registration, residual statistics and the report

**Out** (and which node owns it instead):
- The solver (owned by `ALN.BFIT.ICP.LOOP`); coarse initial pose (owned by `ALN.PRE`)
- N-scan simultaneous adjustment (owned by `SCAN.CLOUD.REG.GLOBAL`)
- Report type and version writer (owned by `SCAN.CLOUD.REG.RREP`)

## Requirements

- **REQ-SCAN.CLOUD.REG.PAIR-001** When `cloud.register_pair@1` runs, the system shall call ALN's `bestfit` with the stride-sampled moving points, an `OrientedCloudTarget` of the fixed scan, the stated metric, stop criteria and maximum correspondence distance, starting from the stated initial pose.
- **REQ-SCAN.CLOUD.REG.PAIR-002** The result shall record the solver's algorithm ID, status and stop reason, and the report shall hold the pair's overlap fraction at overlap distance d and the RMS and maximum absolute point-to-plane residual in metres at the final pose.
- **REQ-SCAN.CLOUD.REG.PAIR-003** When the solver returns status `converged`, the command shall write a registered version of the moving scan through `write_registered`, and shall not create a version for the fixed scan.
- **REQ-SCAN.CLOUD.REG.PAIR-004** If the solver returns `not_converged`, then the command shall create no element unless the parameter `accept_not_converged` is true, and shall return the report in either case.
- **REQ-SCAN.CLOUD.REG.PAIR-005** If the two scans are in different frames and no initial pose maps between them, then the command shall fail with `geo.frame.mismatch` and create no element.
- **REQ-SCAN.CLOUD.REG.PAIR-006** On fixture `three_scans_known` with noise σ = 10 µm and a start within 2 mm and 2°, the recovered relative pose shall lie within 5 µm and 2e-5 rad of the true pose [unverified].
- **REQ-SCAN.CLOUD.REG.PAIR-007** The command shall be determinism class D0, inheriting ALN's D0 declaration.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/reg/three_scans_known`, scans 0 (fixed) and 1 (moving), when `fid cloud register-pair --moving <s1> --fixed <s0> --metric point_to_plane --json` runs, then the pose error meets REQ-006, one registered version of s1 is created with a marker, and s0 and s1 inputs are unchanged.
- **AC-2** Given the same run, the report lists overlap in [0.3, 0.4] (fixture design value 0.35), RMS within 2σ of the noise level, and the ALN algorithm ID `align.bestfit.point_to_plane@1.0.0`.
- **AC-3** Given `--max-iterations 1` from a 5° start, then status is `not_converged`, no element is created, and the report is returned.
- **AC-4** (error path) Given scans in two unrelated frames and no initial pose, the command fails with `geo.frame.mismatch` and journals nothing.
- **AC-5** (replay) Given the journal of AC-1, replay in exact mode gives `IDENTICAL`.

## Design

- **Registry command:** `cloud.register_pair@1`, W2, D0. Params: `moving`, `fixed` (cloud handles), `initial` (`identity` | `transform` 4×4 | `alignment` handle), `metric` (`point_to_plane` default | `point_to_point`), `max_corr_distance` (m), `max_samples` (default 200 000), `overlap_distance` (m, default 3 × median point spacing of the fixed scan, recorded), `stop` (`StopCriteria` from ALN.BFIT.STOP), `dof_locks?`, `accept_not_converged` (default false). Result: report digest, new version handle or none.
- **Algorithm:** resolve the initial pose (alignment transform read through ALN.MODEL.ELEM); build `OrientedCloudTarget(fixed)`; sample moving points by `stride_sample`; `bestfit(sample, &target, params, start)`; compute overlap and residual statistics on the sample at the final pose; build the report; `write_registered`.
- **Errors:** `geo.frame.mismatch`, `scan.cloud.no_normals`, ALN errors passed through (`insufficient_correspondences`, `nan_in_iteration`).
- **Determinism:** D0.
- **Performance budget:** two 10⁷-point scans, 200 000 samples, under 10 s [unverified].
- **Crate:** `fid-scan-cloud`.

## Test fixtures

`fixtures/scan/reg/three_scans_known` (shared), CC0.

## Build prompt

> In the Rust crate `fid-scan-cloud`, register `cloud.register_pair@1` (W2, D0; CLI `fid cloud register-pair --moving <h> --fixed <h> [--initial identity|<4x4>|--alignment <h>] [--metric point_to_plane|point_to_point] [--max-corr-distance M] [--max-samples N] [--overlap-distance M] [--max-iterations N] [--accept-not-converged] [--json]`, MCP `cloud_register_pair`). Resolve the start pose; build `OrientedCloudTarget` of the fixed scan (module `reg::target`); stride-sample the moving scan; call `fid_align_solve::bestfit(measured, reference, params, start, cancel)` from ALN.BFIT.ICP.LOOP; compute overlap (`overlap_fraction`) and point-to-plane RMS and max residual at the final pose; build `RegistrationReport` (module `reg::report`) recording ALN's algorithm ID, status and stop reason; when converged (or `accept_not_converged`), call `write_registered` for the moving scan only. Frame mismatch without an initial pose fails `geo.frame.mismatch` and creates nothing.
>
> Constraints: CONSTITUTION §3 (generated CLI/MCP, `--json`), §8 (no panic/unwrap/unsafe), KNP-1; inputs never modified. Tests: AC-1..AC-5.

## Open questions

- If ALN later offers a general data-to-data alignment command, this command may become a thin wrapper over it (parent question).

## Children

_Leaf: none._
