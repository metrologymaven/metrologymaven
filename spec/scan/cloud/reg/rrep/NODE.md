---
id: SCAN.CLOUD.REG.RREP
title: Registration report and registered cloud versions
level: 4
parent: SCAN.CLOUD.REG
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-cloud]
built_pr: 682
satisfies: [REQ-SCAN.CLOUD.REG-001, REQ-SCAN.CLOUD.REG-005]
covers: []
depends_on: [GEO.FRAME.APPLY, GEO.FRAME.XFORM, GEO.TYPES.POINTS, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-27
---

# SCAN.CLOUD.REG.RREP: Registration report and registered cloud versions

## Purpose

The common output of every registration command: the `RegistrationReport` (per-scan transform, per-pair overlap and residuals, global residual) and the writer that turns computed poses into new cloud versions in the reference frame, each carrying a `registered` marker. Downstream steps (multi-scan meshing, merging) test that marker instead of trusting that clouds happen to share a frame.

## Scope

**In:**
- `RegistrationReport` type and its RFC 8785 canonical JSON
- `write_registered(inputs, poses, report)`: one new cloud version per moved scan, transformed into the reference frame
- `RegisteredMarker { group_id, report_digest, pose, reference_frame }` stored as element metadata

**Out** (and which node owns it instead):
- Computing poses (owned by `SCAN.CLOUD.REG.PAIR` and `SCAN.CLOUD.REG.GLOBAL`)
- Overlap computation (owned by `SCAN.CLOUD.REG.CTGT`)
- Element storage and journaling (owned by `CORE`)

## Requirements

- **REQ-SCAN.CLOUD.REG.RREP-001** For each registered scan, the writer shall create a new cloud version whose points and normals are the input's transformed by the computed pose into the reference frame, which references the input version digest, and which carries a `RegisteredMarker` with group ID, report digest and pose.
- **REQ-SCAN.CLOUD.REG.RREP-002** The writer shall leave every input cloud version bit-identical.
- **REQ-SCAN.CLOUD.REG.RREP-003** The report shall list per scan the 4×4 transform and its translation (m) and rotation angle (rad), per pair the overlap fraction and the RMS and maximum absolute residual in metres, and the global RMS residual in metres, with the solver algorithm ID and convergence status.
- **REQ-SCAN.CLOUD.REG.RREP-004** The report's canonical JSON shall be byte-identical for identical inputs, and its SHA-256 digest shall be the value stored in each marker.
- **REQ-SCAN.CLOUD.REG.RREP-005** The group ID shall be a UUIDv5 derived from the sorted input version digests and the command ID.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/reg/three_scans_known` and known poses, when `write_registered` runs, then each output's points equal `transform_element` of the input bit-for-bit, each marker holds the report digest, and input digests are unchanged.
- **AC-2** Given the same inputs twice, then report bytes and group ID are identical.
- **AC-3** Given a pose for the fixed scan equal to identity, then that scan still gets a new version with a marker (so all members are marked) and identical coordinates.
- **AC-4** (error path) Given a non-rigid transform, the writer fails with `geo.frame.not_rigid` (from GEO.FRAME.XFORM) and creates nothing.

## Design

- **API** (`fid-scan-cloud::reg::report`): `RegistrationReport { scans: [ScanPose { scan, version_digest, transform, translation_m, rotation_rad }], pairs: [PairStat { a, b, overlap, rms_m, max_m, n }], global_rms_m, algorithm_id, status, stop_reason }`; `fn write_registered(inputs: &[CloudVersionRef], poses: &[Rigid], report: &RegistrationReport, reference: FrameRef) -> Result<Vec<NewCloudVersion>, RegError>`.
- **Algorithm:** canonical JSON via CORE.JRN.CANON; transform via GEO.FRAME.APPLY `transform_element`; rotation angle from the rotation matrix trace with `fid-num` `acos` on a clamped value.
- **Errors:** `geo.frame.not_rigid`, `scan.cloud.invalid_parameter` (pose count mismatch).
- **Determinism:** D0.
- **Crate:** `fid-scan-cloud`.

## Test fixtures

`fixtures/scan/reg/three_scans_known` (seeded synthetic scans of a freeform part with known poses; shared with PAIR, GRAPH, GLOBAL), CC0.

## Build prompt

> In the Rust crate `fid-scan-cloud`, module `reg::report`, implement `RegistrationReport` (per-scan transform, translation in m, rotation angle in rad; per-pair overlap, RMS and max residual in m with sample count; global RMS; solver algorithm ID, status and stop reason), serialised as RFC 8785 canonical JSON (`fid-jrn-canon`) with its SHA-256 digest. Implement `write_registered(inputs, poses, report, reference)` that creates one new cloud version per input by `transform_element` (GEO.FRAME.APPLY) into the reference frame, records the input digest, and attaches `RegisteredMarker { group_id (UUIDv5 of sorted input digests plus command ID), report_digest, pose, reference_frame }`. Inputs are never modified; non-rigid poses fail `geo.frame.not_rigid`.
>
> Constraints: CONSTITUTION §5 (derived IDs UUIDv5), §8 (no panic/unwrap/unsafe), KNP-1. Tests: AC-1..AC-4.

## Open questions

- **Build notes (fid-scan-cloud `reg::report`, 2026-11-27).** These API choices differ from the Design above. All are additive.
  - **Canonical JSON from `fid-kci` instead of CORE.JRN.CANON (`fid-canon` / `fid-jrn-canon`).** `fid-scan-cloud` is a kernel crate and `fid-canon` is in the model layer, so the kernel cannot reach it. The report uses `fid-kci`'s kernel canonical JSON instead: RFC 8785 restricted to strings, arrays and objects, with every number written as a string (an f64 as hex bits). SHA-256 also comes from `fid-kci`.
  - **The report is built from the inputs and poses.** The signature is `RegistrationReport::new(command, inputs, poses, reference, graph, SolverSummary { algorithm, verdict, pairs, global_rms_m })`. So the per-scan version digest, translation and rotation cannot disagree with the inputs. The version digest is the GEO.TYPES.DIGEST content digest, computed here and never taken from the caller.
  - **`write_registered(inputs, poses, report, reference, accept_not_converged)` re-checks inputs, poses and reference against the report.** It refuses a `not_converged` report with `scan.cloud.not_converged` unless `accept_not_converged` is true (maintainer decision, matching SCAN.CLOUD.REG.PAIR-004). `RegisteredMarker` always carries the solve status.
  - **Input and pose types.** Inputs are `GeometryElement`s (point clouds only, in one shared frame) instead of a `CloudVersionRef`, because CORE storage is out of scope here. Poses are `Transform`s, so that AC-4 is reachable: a similarity is `geo.frame.not_rigid`. `rigid_from_matrix4` refuses a 4 x 4 matrix that is not rigid with the same code.
  - **Each `PairStat` is built from the CTGT `Overlap` and its overlap distance.** It records `overlap_distance_m`, `within`, `sampled` and the fraction, plus RMS, maximum, `n` and the required `no_residual` counts (`no_hit`, `no_normal`, `rejected`).
  - **The report takes the `PairGraph`.** It records it as the required `graph` member: overlap distance, minimum overlap, sample cap, components, and every unordered pair with its two fractions and status `edge`, `below_min` or `disjoint`. Every pair statistic must be a graph edge. The graph's link to the inputs is the caller's, because recomputing it here would repeat the GRAPH work; the report checks only its size, its connectivity, and that every pair is an edge.
  - **The report JSON (`fid.scan.registration-report/1`) adds required members.** `command` (the group ID hashes it), `reference_frame`, `graph`, `schema`, `accuracy_basis: unverified` and `residual_kind: registration_fit` (D-045: these are fit residuals, not a verified measurement uncertainty). Every member is required in 1.0.
  - **Translation and status.** `translation_m` is read as the length of the translation vector; the vector itself is in the 4 x 4 transform. The status is `converged` or `not_converged`, derived from the ALN.BFIT.STOP verdict, whose text is the stop reason.
  - **Extra refusals.** The report refuses fewer than 2 scans, a pair with no residual samples, a repeated pair, a pair that is not a graph edge, and a scan that appears in no pair. RMS <= maximum is not enforced, because a correctly computed RMS can round a few ulps above the maximum.
- **Rotation-angle resolution.** The trace/acos angle pinned above cannot resolve rotations below about 1.5e-8 rad: the largest `c < 1` is `1 - 2^-53`, whose angle is `sqrt(2 * 2^-53)`. The same floor applies near pi. The angle is a summary for people; consumers must use the 4 x 4 transform. The angle comes from `fid_num::f::acos`, a faithfully rounded libm port that is bit-identical across platforms. The oracle therefore checks it within 1 ulp of the correctly rounded value and compares every other report byte exactly.
- **Conflict with SCAN.CLOUD.REG.PAIR-003.** PAIR says the command "shall not create a version for the fixed scan". This leaf's AC-3 ("so all members are marked") and SCAN.POLY.MULTI.CMD-002 need the fixed scan marked too. Suggested resolution, for PAIR to adopt: pass both scans to `write_registered`; the fixed scan gets an identity-pose version.
- How CORE stores the `RegisteredMarker` as element metadata (its canonical form) is still to be defined by the consuming command leaves.

## Children

_Leaf: none._
