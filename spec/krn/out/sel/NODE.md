---
id: KRN.OUT.SEL
title: Point-selection command
level: 3
parent: KRN.OUT
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.OUT-008, REQ-KRN.OUT-009]
covers: [PW.CON.06]
depends_on: [KRN.OUT.POL, KRN.OUT.DISC]
side_effect: W2
determinism: D0
cli: [fid fit select-points]
mcp: [fit_select_points]
journal: [fit.select_points@1]
updated: 2026-11-16
---

# KRN.OUT.SEL: Point-selection command

## Purpose

Chooses which points of an input set a fit uses, by an index set already resolved on the CPU (from a GUI pick or lasso, or an agent's semantic query) or by extraction source (probing hits, section points, surface-patch points, as tagged by the producing domain). The selection is recorded like any exclusion, so a fit on a subset is always auditable.

## Scope

**In:** the `fit.select_points@1` command; selection by explicit index set, by index ranges, and by source tag; validation that every reference is inside the input; the resulting `ExclusionRecord` (complement of the selection).
**Out:** resolving picks, lassos and semantic queries into index sets (`VIZ`, `GEO.SEL`); producing tagged point sets (`SCAN`, `HAL`, `CMM`); outlier policies (`KRN.OUT.RUN`).

## Requirements

- **REQ-KRN.OUT.SEL-001** When `fit.select_points` runs, the system shall produce a selection from an explicit index set, a list of index ranges, or a set of source tags, given as exactly one of these.
- **REQ-KRN.OUT.SEL-002** If any index or range refers to a point outside the input set, or a source tag is absent from the input, then the command shall fail with a typed error naming the first offending reference and journal nothing.
- **REQ-KRN.OUT.SEL-003** The result shall be a selected-index set in ascending order plus an `ExclusionRecord` for its complement with policy ID `filter.select.<method>@<semver>`.
- **REQ-KRN.OUT.SEL-004** When the command is committed, the system shall journal one `fit.select_points@1` event with the selection method, its parameters, the excluded-set digest and the output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/out/sel-tagged-v1` (3 000 points tagged `probe`, `section` and `surface`), when `fid fit select-points --source section --json` runs, then exactly the 1 000 section-tagged indices are selected in ascending order and the complement record lists the other 2 000.
- **AC-2** Given index ranges [0..10) and [20..25), then 15 indices are selected.
- **AC-3** (error path) Given index 3 000 for 3 000 points, then the command fails with `IndexOutOfRange{index: 3000}` and journals nothing; given two selection modes at once, it fails with `InvalidParameter`.
- **AC-4** (replay) Given the committed entry from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.select_points@1`, class W2 (dry-run R1), determinism D0; params: point-set handle, one of {index set, index ranges, source tags}; result: selected indices, `ExclusionRecord`.
- **Data structures:** `enum Selection { Indices(Vec<u32>), Ranges(Vec<(u32, u32)>), Sources(Vec<SourceTag>) }`; source tags come with the `PointsView` as a parallel u16 array defined by the producing domain.
- **Algorithm:** validation, then set construction with sorting and deduplication.
- **Errors:** `PolicyError::{IndexOutOfRange, UnknownSource, InvalidParameter}`.
- **Determinism:** D0.
- **Performance budget:** 1 M points < 20 ms [unverified until measured].
- **Crate:** `fid-filter` (module `select`), plus the command data row.

## Test fixtures

`fixtures/out/sel-tagged-v1`: synthetic tagged points, CC-BY-4.0.

## Build prompt

> In crate `fid-filter`, create module `select` with `pub fn select_points(pts: &PointsView, tags: Option<&[u16]>, sel: &Selection) -> Result<(Vec<u32>, ExclusionRecord), PolicyError>`, where `Selection` is exactly one of explicit indices, index ranges or source tags. Validate every reference (typed errors with the first offending reference), sort and deduplicate, and build the complement `ExclusionRecord` (module `disclose`) with policy ID `filter.select.<method>@1.0.0`. Add the registry data row for `fit.select_points@1` (W2, dry-run R1, D0; CLI `fid fit select-points`; MCP `fit_select_points`; journal one event with method, parameters, excluded-set digest and output digest). Tests: `fixtures/out/sel-tagged-v1` source selection, ranges, out-of-range and mixed-mode errors with no journal entry, exact replay IDENTICAL. KNP-1: no FMA, no HashMap/HashSet (use sorted vectors or BTreeSet), no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.OUT.SEL.

## Open questions

- The source-tag vocabulary is owned by the producing domains; agree a shared u16 registry with `GEO.TYPES` (per-point attributes).

## Children

_Leaf: none._
