---
id: KRN.CONS.OPS.TABLE
title: Support table, dispatch and the six operation commands
level: 4
parent: KRN.CONS.OPS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.OPS-001, REQ-KRN.CONS.OPS-002, REQ-KRN.CONS.OPS-005]
covers: [PCD.CON.01, PW.CON.04]
depends_on: [KRN.CONS.DECL, KRN.CONS.OPS.INTLIN, KRN.CONS.OPS.INTQUAD, KRN.CONS.OPS.PROJ, KRN.CONS.OPS.OFFS, KRN.CONS.OPS.MID, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct intersect-elements, fid construct project-element, fid construct offset-element, fid construct midpoint-elements, fid construct symmetry-elements, fid construct bisect-elements]
mcp: [construct_intersect_elements, construct_project_element, construct_offset_element, construct_midpoint_elements, construct_symmetry_elements, construct_bisect_elements]
journal: [construct.intersect_elements@1, construct.project_element@1, construct.offset_element@1, construct.midpoint_elements@1, construct.symmetry_elements@1, construct.bisect_elements@1]
updated: 2026-11-17
---

# KRN.CONS.OPS.TABLE: Support table, dispatch and the six operation commands

## Purpose

The user-facing side of the construction operations. A single support table, served as data, says which (operation, input kind, input kind) triples are defined and which algorithm implements each; the six registry commands look up the table, dispatch to the variant in the sibling leaves, and return `Undefined{reason: unsupported_pair}` for anything not listed. `AUTO` docs and `VIZ` menus read the same table.

## Scope

**In:** the support table (operation, ordered input kinds, algorithm ID, required parameters) as a sorted static array and as canonical JSON for consumers; kind normalisation (for example a point-like centre); dispatch; the six command data rows; spec cards for the commands' dispatch.
**Out:** the geometric variants (`KRN.CONS.OPS.INTLIN`, `KRN.CONS.OPS.INTQUAD`, `KRN.CONS.OPS.PROJ`, `KRN.CONS.OPS.OFFS`, `KRN.CONS.OPS.MID`); menus and docs that render the table (`VIZ`, `AUTO`).

## Requirements

- **REQ-KRN.CONS.OPS.TABLE-001** The module shall publish one support table listing every defined (operation, input kinds) triple with its algorithm ID and required parameters, sorted by a total order, as data and as canonical JSON with a digest.
- **REQ-KRN.CONS.OPS.TABLE-002** When an operation command runs, the system shall dispatch to the algorithm the table names for the inputs' kinds in the given order, trying the swapped order only where the table marks the operation symmetric.
- **REQ-KRN.CONS.OPS.TABLE-003** If no table row matches, then the command shall return `Undefined{reason: unsupported_pair, op, kinds}` and journal nothing.
- **REQ-KRN.CONS.OPS.TABLE-004** CI shall fail if any table row names an algorithm without a spec card or without an implementation, or if any implemented variant is missing from the table.
- **REQ-KRN.CONS.OPS.TABLE-005** When a command is committed, the system shall journal one `construct.<verb_object>@1` event with the dispatched algorithm ID, input element IDs, parameter digest and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/ops-table-v1` (one case per table row), when each of the six commands runs with `--json`, then the dispatched algorithm ID equals the row's and the result equals a direct call of the variant bit for bit.
- **AC-2** Given `fid construct intersect-elements` with a sphere then a line (symmetric operation), then it dispatches to `line_sphere` with inputs swapped and records that order in the journal.
- **AC-3** (error path) Given `fid construct bisect-elements` on a point and a sphere, then it returns `Undefined{reason: unsupported_pair}` and journals nothing.
- **AC-4** Given the table, when the CI table check runs, then every row has a card and an implementation and every implemented variant has a row.
- **AC-5** Given a committed AC-1 case, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `construct.intersect_elements@1`, `construct.project_element@1`, `construct.offset_element@1`, `construct.midpoint_elements@1`, `construct.symmetry_elements@1`, `construct.bisect_elements@1`; all W2 (dry-run R1). Params: ordered input element handles, variant parameters (`rule`, `branch`, `distance` with unit, `allow_intersecting`), thresholds. Result: the constructed element and diagnostics.
- **Data structures:** `SupportRow { op, kinds: [Kind; 2], symmetric: bool, algorithm: AlgorithmId, required_params: &'static [ParamName] }`; table sorted by (op, kinds) for binary search.
- **Algorithm:** normalise kinds, binary-search the table, swap if symmetric, call the variant's `Construction`.
- **Errors:** `Undefined{reason: unsupported_pair}`, `InputCount`, errors from the variant.
- **Determinism:** D0.
- **Performance budget:** dispatch < 0.2 microseconds [unverified until measured].
- **Crate:** `fid-construct` (module `ops_table`) plus the six command data rows.

## Test fixtures

`fixtures/cons/ops-table-v1`: one generated case per table row, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `ops_table`: a sorted static support table of `SupportRow { op, kinds, symmetric, algorithm, required_params }` covering every variant in `ops_intlin`, `ops_intquad`, `ops_proj`, `ops_offs` and `ops_mid`, also emitted as canonical JSON with a digest; dispatch by binary search, swapping inputs only for symmetric operations; `Undefined{reason: unsupported_pair, op, kinds}` otherwise. Add registry data rows for `construct.intersect_elements@1`, `construct.project_element@1`, `construct.offset_element@1`, `construct.midpoint_elements@1`, `construct.symmetry_elements@1`, `construct.bisect_elements@1` (W2, dry-run R1, D0; CLI `fid construct <verb-object>`; MCP `construct_<verb_object>`; one journal event each with dispatched algorithm ID, input element IDs, parameter digest, output digest). Add a CI test that every row has a spec card and an implementation and every variant has a row. Tests: `fixtures/cons/ops-table-v1` dispatch equality, symmetric swap, unsupported pair, table completeness, exact replay IDENTICAL. Deny `unwrap`/`expect`/`panic`; no HashMap/HashSet. Acceptance: AC-1..AC-5 of spec node KRN.CONS.OPS.TABLE.

## Open questions

- None.

## Children

_Leaf: none._
