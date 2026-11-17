---
id: SCAN.CLOUD.MGMT
title: Cloud grouping, splitting and merging
level: 3
parent: SCAN.CLOUD
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CLOUD-001, REQ-SCAN.CLOUD-007]
covers: [SA.SCN.01, PCD.LAS.02]
depends_on: [GEO.TYPES, GEO.SEL, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid cloud group-clouds, fid cloud split-cloud, fid cloud merge-clouds]
mcp: [cloud_group_clouds, cloud_split_cloud, cloud_merge_clouds]
journal: [cloud.group_clouds@1, cloud.split_cloud@1, cloud.merge_clouds@1]
updated: 2026-11-16
---

# SCAN.CLOUD.MGMT: Cloud grouping, splitting and merging

## Purpose

The point-cloud management operations users expect from large-volume and laser-scanning tools: collect clouds into named groups, split one cloud into several, and merge several into one, while keeping track of which scan each point came from. Per-point scan provenance is what lets later steps (registration, meshing overlap reduction, normal orientation toward a sensor) treat each scan separately.

## Scope

**In:**
- Cloud groups: an element that lists member cloud versions in order
- Split by selection (selected and remainder), by scan provenance value, or by a regular spatial grid
- Merge of two or more clouds in the same frame into one cloud, in stated order
- Per-point `scan_id` provenance attribute: created on merge when absent, preserved on split

**Out** (and which node owns it instead):
- Renaming, deleting and tree placement of elements (owned by `CORE`)
- Registration of scans before merging (owned by `SCAN.CLOUD.REG`)
- Frame conversion of clouds (owned by `GEO.FRAME`)
- Selection storage and resolution (owned by `GEO.SEL`)

## Requirements

- **REQ-SCAN.CLOUD.MGMT-001** When a split or merge command runs, the system shall create new cloud element versions that reference the digests of all input versions and shall leave every input element bit-identical.
- **REQ-SCAN.CLOUD.MGMT-002** When `cloud.group_clouds@1` runs, the system shall create a group element that lists member cloud versions by ID and digest in the stated order, without copying point data.
- **REQ-SCAN.CLOUD.MGMT-003** When a cloud is split by selection, the system shall produce two clouds whose point sets are exactly the selected and the unselected points, each in input index order.
- **REQ-SCAN.CLOUD.MGMT-004** When a cloud is split by scan provenance, the system shall produce one cloud per distinct `scan_id` value, ordered by ascending `scan_id`.
- **REQ-SCAN.CLOUD.MGMT-005** When a cloud is split by spatial grid with cell size in metres, the system shall produce one cloud per occupied cell, ordered by Morton order of the cell index.
- **REQ-SCAN.CLOUD.MGMT-006** When clouds are merged, the system shall concatenate their points in the stated input order, keep every attribute present in all inputs, and set a `scan_id` per point equal to the source cloud's existing value or, if absent, to a new scan ID derived as a UUIDv5 from the source element ID.
- **REQ-SCAN.CLOUD.MGMT-007** If merge inputs are in different frames or have different attribute types for the same attribute name, then the system shall fail with a typed error and create no element.
- **REQ-SCAN.CLOUD.MGMT-008** Group, split and merge shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cloud_three_scans` (three 10⁴-point scans of one part in the same frame, each with a sensor origin, seed 21), when `fid cloud merge-clouds --clouds a,b,c --json` runs, then the merged cloud has 3·10⁴ points, three distinct `scan_id` values in input order, and splitting it by scan returns three clouds whose point data equal the originals byte for byte.
- **AC-2** Given the merged cloud and a selection of 1 234 points, when `fid cloud split-cloud --by selection` runs, then the outputs hold 1 234 and 28 766 points with no point in both.
- **AC-3** Given `fid cloud split-cloud --by grid --cell 0.05`, then the union of the outputs equals the input point multiset and outputs are in Morton order.
- **AC-4** Given `fid cloud group-clouds --clouds a,b,c --name scans`, then the group element lists three members with digests and no point data is duplicated in the store.
- **AC-5** (error path) Given two clouds in different frames, merge fails with `geo.frame.mismatch` and journals nothing.
- **AC-6** (replay) Given the journal from AC-1, exact-mode replay yields `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `cloud.group_clouds@1` (`clouds` [handles], `name`); `cloud.split_cloud@1` (`cloud`, `by` `selection` | `scan` | `grid`, `selection?`, `cell?` m); `cloud.merge_clouds@1` (`clouds` [handles], ordered). Results list the new element versions.
- **Data:** `CloudGroup { members: [(ElementId, Digest)] }`; `scan_id` per-point attribute (u128 UUID) in GEO.TYPES attributes; the sensor origin of each scan is carried as element metadata keyed by `scan_id`.
- **Errors:** `geo.frame.mismatch` (from GEO.FRAME), `scan.cloud.attribute_conflict`, `scan.cloud.stale_selection` (from GEO.SEL), `scan.cloud.invalid_parameter`.
- **Determinism:** D0; derived IDs are UUIDv5 (CONSTITUTION §5).
- **Performance budget:** linear in point count; merge of 10⁸ points within 30 s [unverified].
- **Crate:** `fid-scan-cloud`.

## Test fixtures

- `fixtures/scan/cloud_three_scans`: synthetic generator, seed 21, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-cloud`, implement three W2, D0 registry commands: `cloud.group_clouds@1` (group element listing member versions by ID and digest, no copying), `cloud.split_cloud@1` (by selection into selected/remainder, by `scan_id` into one cloud per value in ascending order, or by spatial grid cell size in metres into one cloud per occupied cell in Morton order) and `cloud.merge_clouds@1` (concatenate in stated order; keep attributes common to all inputs; assign `scan_id` from the source or a UUIDv5 of the source element ID; carry each scan's sensor origin as metadata). CLI `fid cloud group-clouds|split-cloud|merge-clouds ... [--json]`; MCP `cloud_group_clouds`, `cloud_split_cloud`, `cloud_merge_clouds`.
>
> Outputs are new element versions referencing input digests; inputs are never modified. Fail with `geo.frame.mismatch` for frame differences and `scan.cloud.attribute_conflict` for attribute type clashes, creating nothing. Constraints: no std HashMap/HashSet (use ordered maps), no panic/unwrap/unsafe, f64 metres.
>
> Tests: merge then split-by-scan round-trips three seeded scans byte for byte; selection split counts; grid split union equals input; group stores no point data; frame mismatch fails and journals nothing; exact replay IDENTICAL.

## Open questions

- Whether groups should be allowed to hold meshes as well as clouds; currently clouds only.

## Children

_Leaf: none._
