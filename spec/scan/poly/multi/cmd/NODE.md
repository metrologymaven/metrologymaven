---
id: SCAN.POLY.MULTI.CMD
title: Polygonize-scans command
level: 4
parent: SCAN.POLY.MULTI
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY.MULTI-001, REQ-SCAN.POLY.MULTI-002, REQ-SCAN.POLY.MULTI-003, REQ-SCAN.POLY.MULTI-004, REQ-SCAN.POLY.MULTI-005]
covers: [PW.PCL.03]
depends_on: [SCAN.POLY.MULTI.OVLP, SCAN.POLY.MULTI.REDUCE, SCAN.POLY.SURF.OOC, SCAN.POLY.SURF.CMD, SCAN.CLOUD.REG.RREP, SCAN.CLOUD.MGMT, GEO.FRAME.APPLY]
side_effect: W2
determinism: D0
cli: [fid mesh polygonize-scans]
mcp: [mesh_polygonize_scans]
journal: [mesh.polygonize_scans@1]
updated: 2026-11-17
---

# SCAN.POLY.MULTI.CMD: Polygonize-scans command

## Purpose

The user-facing `mesh.polygonize_scans@1`: check that the scans are registered and share a frame, detect and reduce overlap, mesh the reduced points with the single-cloud reconstructor under the same gate and fidelity rules as `mesh.polygonize_cloud@1`, and report each scan's contribution.

## Scope

**In:**
- Inputs: two or more registered scan clouds, or one merged cloud with `scan_id` provenance (split virtually by `scan_id`)
- Registration check via `RegisteredMarker` (same group ID) and frame check
- Composition OVLP → REDUCE → `reconstruct`, with the SURF gate and fidelity check
- Result: mesh, `ScanContribution` rows, rule and parameters, quality report

**Out** (and which node owns it instead):
- Overlap and reduction (owned by OVLP and REDUCE); reconstruction (owned by `SCAN.POLY.SURF`)

## Requirements

- **REQ-SCAN.POLY.MULTI.CMD-001** When `mesh.polygonize_scans@1` runs, the system shall create one mesh element referencing the digests of every input cloud version and recording the reduction rule, overlap distance and SURF parameters, and shall leave all inputs bit-identical.
- **REQ-SCAN.POLY.MULTI.CMD-002** If the inputs are in different frames, or any input lacks a `RegisteredMarker` or the markers name different group IDs, then the command shall fail with `scan.poly.not_registered` (or `geo.frame.mismatch`) and create no mesh; a merged cloud with `scan_id` provenance shall carry the marker itself.
- **REQ-SCAN.POLY.MULTI.CMD-003** On fixture `two_sheets_overlap`, the output shall contain no second mesh sheet within the overlap distance along the surface normal anywhere in the strip.
- **REQ-SCAN.POLY.MULTI.CMD-004** The result shall include per-scan kept and discarded counts, and the command shall be D0.

## Acceptance criteria

- **AC-1** Given the two registered scans of `two_sheets_overlap`, when `fid mesh polygonize-scans --scans <a>,<b> --rule local_noise --overlap-distance "0.3 mm" --resolution "0.5 mm" --max-gap "1 mm" --json` runs, then a ray along the normal through 100 sampled strip points hits the mesh exactly once within ±0.3 mm, and contributions are reported.
- **AC-2** Given the same scans merged by `cloud.merge_clouds@1` after registration, the result equals AC-1's mesh digest.
- **AC-3** (error path) Given one unregistered scan, the command fails `scan.poly.not_registered` and journals nothing.
- **AC-4** (replay) Replay of AC-1 in exact mode is `IDENTICAL`.

## Design

- **Registry command:** `mesh.polygonize_scans@1` (W2, D0): `scans` [handles] or `cloud` (merged), `rule` (required), `overlap_distance` (length), plus the `mesh.polygonize_cloud@1` parameters.
- **Algorithm:** marker and frame checks; virtual split by `scan_id` for merged input; `detect_overlap`; `reduce`; build a reduced point view (no copy, index sets in scan order); `ooc::reconstruct`; fidelity and gate checks shared with SURF.CMD; `mesh_quality`.
- **Errors:** `scan.poly.not_registered`, `geo.frame.mismatch`, `scan.poly.rule_required`, SURF errors passed through.
- **Determinism:** D0.
- **Crate:** `fid-scan-poly`.

## Test fixtures

`two_sheets_overlap` (shared), CC0.

## Build prompt

> In the Rust crate `fid-scan-poly`, register `mesh.polygonize_scans@1` (W2, D0; CLI `fid mesh polygonize-scans --scans <h>,<h>...|--cloud <h> --rule incidence_angle|sensor_distance|local_noise --overlap-distance L --resolution L --max-gap L [...] [--json]`, MCP `mesh_polygonize_scans`). Require a common frame and a `RegisteredMarker` with one group ID on every input (from SCAN.CLOUD.REG; a merged cloud carries its own) else fail `scan.poly.not_registered`; split merged input by `scan_id`; run `multi::overlap::detect_overlap`, `multi::reduce::reduce`, then `ooc::reconstruct` on the kept points with the same normal gate and fidelity check as `mesh.polygonize_cloud@1`; attach the quality report and per-scan contributions; record all parameters and input digests. Constraints: §3, §8, KNP-1. Tests: AC-1..AC-4.

## Open questions

- A merged cloud keeps the marker only if `cloud.merge_clouds@1` (SCAN.CLOUD.MGMT) propagates it when all inputs share one group ID; that is a change for MGMT's owner.

## Children

_Leaf: none._
