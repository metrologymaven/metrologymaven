---
id: VIZ.SHELL.SPKTABLE
title: Toolkit spike - characteristic table
level: 3
parent: VIZ.SHELL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-003, REQ-VIZ.SHELL-004]
covers: []
depends_on: []
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.SHELL.SPKTABLE: Toolkit spike - characteristic table

## Purpose

The second D-005 spike: a dense characteristic and results table in egui (the screen metrology users live in) with many rows, frozen columns, sorting, filtering, in-place editing of nominal and tolerance cells with exact decimal entry, status chips and full keyboard and screen-reader support. It reports whether egui meets fixed criteria; a human then confirms or reopens D-005.

## Scope

**In:** a prototype table of 10⁵ rows × 14 columns (concept-doc scale for results tables) with virtual scrolling, frozen first two columns, sort, filter, column resize and reorder, cell editing, copy to clipboard as TSV, row-to-viewport fly-to stub; benchmark harness; results report.
**Out:** the production results table (`VIZ.SHELL.TREE`); the other spikes; the decision (maintainers, D-005).

## Requirements

- **REQ-VIZ.SHELL.SPKTABLE-001** The spike shall render a table of 10⁵ rows and 14 columns with virtual scrolling, two frozen columns and per-row status chips with icon and text.
- **REQ-VIZ.SHELL.SPKTABLE-002** The spike shall pass only if p95 frame time stays under 33 ms on the H1 profile while scrolling, sorting a column and typing a filter [unverified].
- **REQ-VIZ.SHELL.SPKTABLE-003** Every table operation (navigate cells, sort, filter, edit, commit, cancel, copy) shall be reachable by keyboard alone.
- **REQ-VIZ.SHELL.SPKTABLE-004** The spike shall verify that a screen reader on Windows announces table, row and column headers, cell value and status text for a focused cell.
- **REQ-VIZ.SHELL.SPKTABLE-005** Edited numeric cells shall keep the typed decimal string exactly, with no binary round trip, and the spike shall publish a results report with pass or fail per criterion.

## Acceptance criteria

- **AC-1** Given generated fixture `fixtures/viz/char-table-100k` (seed 9), when the harness scrolls end-to-end for 30 s on H1, then p95 frame time is recorded and judged against 33 ms.
- **AC-2** Given the same fixture, when the harness sorts by deviation and types a 6-character filter, then p95 frame time during each operation is recorded and judged.
- **AC-3** Given a cell edited to "0.0500", when committed, then the stored string is exactly "0.0500".
- **AC-4** Given the scripted keyboard sequence in the fixture, then every listed operation completes without pointer input.
- **AC-5** Given NVDA or Narrator, when focus moves across a row, then the manual log records headers, value and status text (manual step recorded in the report); any failure leads to "fail" and a pointer to D-005 option B for this screen.

## Design

- **Registry command(s):** none (prototype).
- **Data structures:** synthetic rows `{ id, balloon, characteristic, nominal, upper, lower, actual, deviation, status, uec, unit, method, feature, note }`, numbers held as decimal strings.
- **Algorithm:** row virtualisation over a sorted index array; sort on a background thread with result swap; frozen columns as a separate clipped region.
- **Errors:** not applicable.
- **Determinism:** N (timings observed).
- **Performance budget:** p95 < 33 ms on H1 [unverified].
- **Crate:** `fid-spike-ui` (example `chartable`; throwaway).

## Test fixtures

`fixtures/viz/char-table-100k`: generated (seed 9), in-repo.

## Build prompt

> In throwaway crate `fid-spike-ui`, add example `chartable`: an egui + winit + wgpu window with a 10⁵-row × 14-column characteristic table (generated, seed 9): virtual scrolling, two frozen columns, sort, filter, column resize and reorder, in-place editing that stores the typed decimal string exactly, status chips with icon and text, TSV copy, and complete keyboard operation; expose cells through AccessKit with row and column headers. Benchmark p95 frame time while scrolling, sorting and filtering on the H1 profile, and write `REPORT.md` with pass/fail per criterion (p95 < 33 ms; keyboard completeness; screen-reader announcements; exact decimal storage), versions and limitations. The report is input to the maintainers' confirmation of D-005; do not adopt it. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; never a dependency of product crates.

## Open questions

- None blocking; H1 caveat as in VIZ.SHELL.SPKTREE.

## Children

_Leaf: none._
