---
id: LVM.STATION.MATCH
title: Common-point matching, renaming and exclusion
level: 3
parent: LVM.STATION
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-LVM.STATION-003, REQ-LVM.STATION-007]
covers: []
depends_on: [LVM.STATION.MODEL, LVM.STATION.OBS]
side_effect: W2
determinism: D0
cli: [fid net match-common-point, fid net rename-common-point, fid net set-common-point-enabled]
mcp: [net_match_common_point, net_rename_common_point, net_set_common_point_enabled]
journal: [net.match_common_point@1, net.rename_common_point@1, net.set_common_point_enabled@1]
updated: 2026-11-16
---

# LVM.STATION.MATCH: Common-point matching, renaming and exclusion

## Purpose

Lets the user tie together observations of the same physical target that were recorded under different names, rename common points, and exclude a common point from the network without deleting its data. Automatic linking by identical names happens at name resolution (`LVM.STATION.MODEL`); this node owns every explicit, journaled change to that linking.

## Scope

**In:**
- Merging a source common point into a target common point (explicit match)
- Renaming a common point, keeping the old name as an alias
- Enabling and disabling a common point (exclusion)
- A warning when a merge joins points that were both observed from the same station

**Out** (and which node owns it instead):
- Name normalisation and implicit linking by equal names (`LVM.STATION.MODEL`)
- Disabling single observations (`LVM.STATION.OBS`)
- Suggesting matches by geometry (not yet assigned; see Open questions)

## Requirements

- **REQ-LVM.STATION.MATCH-001** When `net.match_common_point@1` merges source point S into target point T, the system shall re-point every observation of S to T, add S's canonical name and aliases to T's aliases, mark S as merged into T, and journal one event.
- **REQ-LVM.STATION.MATCH-002** When a merge joins two points that were both observed from at least one common station, the system shall complete the merge and return a `NET_MERGE_STATION_OVERLAP` warning listing those stations.
- **REQ-LVM.STATION.MATCH-003** If source and target are the same point, or either is unknown or already merged, then the command shall fail with `NET_MATCH_INVALID` and journal nothing.
- **REQ-LVM.STATION.MATCH-004** When `net.rename_common_point@1` runs, the system shall set the new canonical name, keep the old name as an alias so later imports under the old name still resolve to the point, and fail with `NET_DUPLICATE_LABEL` if the new name is already used.
- **REQ-LVM.STATION.MATCH-005** When `net.set_common_point_enabled@1` disables a point, the system shall keep the point and all its observations unchanged, and every consumer that reads enabled data shall skip that point and its observations.
- **REQ-LVM.STATION.MATCH-006** Undoing a merge, rename or enable change through the journal shall restore the exact prior point set, aliases and observation links.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/net-names-mixed` (2 stations; station A names targets `P1..P8`, station B names the same targets `T01..T08`), when `fid net match-common-point --source T01 --target P1 --json` runs for all 8 pairs, then the network has 8 enabled common points each observed from 2 stations, and `T01` resolves to `P1`.
- **AC-2** (warning) Given the same fixture, when `P2` is merged into `P1`, then the merge succeeds and the result carries `NET_MERGE_STATION_OVERLAP` naming station A.
- **AC-3** (error path) Given any point, when it is merged into itself, then the command fails with `NET_MATCH_INVALID` and the journal length is unchanged.
- **AC-4** (rename) Given `P3` renamed to `NEST-3`, when a new observation under name `P3` is added, then it links to `NEST-3`.
- **AC-5** (exclusion) Given `P4` disabled, when `fid net list-observations --enabled true --json` runs, then no observation of `P4` is listed, and `P4`'s observation payloads are byte-identical to before.
- **AC-6** (undo and replay) Given the journal from AC-1 to AC-5, when each step is undone in reverse, then the network equals the fixture; when the full journal is replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands (all W2):**
  - `net.match_common_point@1`: `network, source: PointRef, target: PointRef` → `MatchResult { moved_observations: u32, warnings }`.
  - `net.rename_common_point@1`: `point: PointRef, new_name: String` → `()`.
  - `net.set_common_point_enabled@1`: `point: PointRef, enabled: bool` → `()`.
  - `PointRef` accepts an ID or a name resolved through the model.
- **Data structures:** `CommonPoint` gains `merged_into: Option<PointId>`; merged points stay in the vector for undo and provenance and are hidden from `net.list_common_points` by default.
- **Algorithm:** merge iterates observations in creation order and rewrites the point ID; overlap detection collects station IDs of both points into sorted vectors and intersects them.
- **Errors:** `NET_MATCH_INVALID`, `NET_DUPLICATE_LABEL`, `NET_UNKNOWN_POINT`; warning `NET_MERGE_STATION_OVERLAP`.
- **Determinism:** D0. No numerics.
- **Crate:** `fid-lvm` (proposed name).

## Test fixtures

- `fixtures/lvm/net-names-mixed`: seeded synthetic generator (seed 2), 2 stations, 8 targets with different naming per station; CC-BY-4.0.

## Build prompt

> In the crate `fid-lvm`, add three W2 registry commands for large-volume common points: `net.match_common_point@1 (source, target)` re-points every observation of the source point to the target, moves the source's name and aliases to the target's aliases and marks the source `merged_into` the target; `net.rename_common_point@1 (point, new_name)` keeps the old name as an alias; `net.set_common_point_enabled@1 (point, enabled)` only flips the flag. CLI `fid net match-common-point|rename-common-point|set-common-point-enabled` with `--json`; MCP names are generated. Errors: `NET_MATCH_INVALID` (same point, unknown or already merged), `NET_DUPLICATE_LABEL`, `NET_UNKNOWN_POINT`; failing commands journal nothing. A merge of two points both observed from one station succeeds with warning `NET_MERGE_STATION_OVERLAP` listing the stations. Use creation-order `Vec`s and sorted vectors, never std HashMap/HashSet. Undo through the journal must restore the exact prior state. No panics, no `unwrap`, no `unsafe` (CONSTITUTION §8). Tests: the six acceptance criteria above with a seeded fixture generator. Determinism class D0.

## Open questions

- Geometry-based match suggestions (for example matching unnamed photogrammetry targets by position after a rough alignment) are not assigned. They would be an R1 proposal command whose results a human adopts (C5); candidate for a later child here.

## Children

_Leaf: none._
