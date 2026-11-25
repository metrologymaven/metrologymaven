---
id: GDT.RES.ROWASME.INV
title: DMT inventory file and coverage check
level: 4
parent: GDT.RES.ROWASME
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWASME-001]
covers: []
depends_on: [GDT.RES.SCHEMA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-gdt]
built_pr: 484
updated: 2026-11-22
---

# GDT.RES.ROWASME.INV: DMT inventory file and coverage check

## Purpose

Makes "which rows must exist" machine-checkable. An inventory file lists, per dialect and phase, the DMT keys a release must contain and the family node that owns each; a coverage check compares a table against it and reports missing keys and keys still `unsupported`. The inventory holds keys only (characteristic and modifier names), never interpretation, so it can be drafted from the P1a scope list and then confirmed by the paid author.

## Scope

**In:** format `dmt-inventory/v1` (dialect, edition set, phase, entries `{key, family, required_status: supported_or_partial | unsupported_ok}`); loader; `coverage(table, inventory) -> Vec<CoverageFinding>`; the committed P1a ASME inventory file `dmt/inventory/asme-y14.5-2018-p1a.json` built from the parent's Scope list.
**Out:** row content (owned by the family children); the release gate that consumes the findings (owned by `GDT.RES.ROWASME.GATE`); the ISO counterpart map (owned by `GDT.RES.ROWISO.STUBS`).

## Requirements

- **REQ-GDT.RES.ROWASME.INV-001** The loader shall accept only `dmt-inventory/v1` files whose keys parse with the DMT key grammar, and fail with `bad_dmt_key` or `inventory_invalid{field}` otherwise.
- **REQ-GDT.RES.ROWASME.INV-002** `coverage` shall report `missing_row{key}` for every inventory key absent from the table and `status_below_required{key, status}` for every key whose status does not meet `required_status`.
- **REQ-GDT.RES.ROWASME.INV-003** Findings shall be sorted by key and be byte-identical for identical inputs.
- **REQ-GDT.RES.ROWASME.INV-004** The committed P1a ASME inventory shall list one key per callout and modifier combination in `GDT.RES.ROWASME`'s Scope, each assigned to exactly one family child (SIZE, FORM, ORIENT, POS, DRF), and shall be confirmed by a paid author through a reviewed pull request before P1a exit.
- **REQ-GDT.RES.ROWASME.INV-005** If an inventory lists the same key twice or names an unknown family, then loading shall fail with `inventory_invalid`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dmt/inventory_synthetic.json` (6 keys) and `synthetic_table_v1.json` (4 of them, one `unsupported`), when `coverage` runs, then it reports two `missing_row` and one `status_below_required` findings in key order.
- **AC-2** Given a table covering every key as `supported`, then zero findings.
- **AC-3** (error path) Given a duplicate key or family `PROFILE`, then `inventory_invalid`.
- **AC-4** Given the committed P1a inventory, then it loads, and every family child of `GDT.RES.ROWASME` owns at least one key.

## Design

- **Registry command(s):** none; library plus a CI test target that loads the committed inventory and the committed DMT and prints findings.
- **Data structures:** `Inventory { dialect, editions, phase, entries: BTreeMap<DmtKey, Entry{family, required_status}> }`; `CoverageFinding { kind, key, detail }`.
- **Algorithm:** one pass over inventory keys in order, looking up the table.
- **Errors:** `bad_dmt_key`, `inventory_invalid{field}`.
- **Determinism:** D0.
- **Performance budget:** 5 000 keys < 50 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_inventory`).

## Test fixtures

Synthetic inventories and tables (CC0, in-repo); the committed P1a inventory (keys only, CC-BY-4.0 with the spec).

## Build prompt

> In crate `fid-gdt`, add module `dmt_inventory`. Define `dmt-inventory/v1` (dialect, edition set, phase, entries keyed by `DmtKey` with `family` ∈ {SIZE, FORM, ORIENT, POS, DRF, DPROF, GEOTOL, DATUM, MATREQ} (the family names are per inventory file, so ASME and ISO inventories share SIZE and DPROF) and `required_status` ∈ {supported_or_partial, unsupported_ok}); loader with errors `bad_dmt_key`, `inventory_invalid{field}` (duplicates, unknown family, unknown field). Implement `coverage(table: &DmtTable, inv: &Inventory) -> Vec<CoverageFinding>` with `missing_row{key}` and `status_below_required{key, status}`, sorted by key, deterministic. Add a CI test target over the committed inventory and DMT. Draft `dmt/inventory/asme-y14.5-2018-p1a.json` with keys only (no clause numbers, no interpretation) from this list: distance, caliper, angle, diameter, radius, scalar, min/max, point deviation, two-point size, envelope (Rule #1), flatness, straightness, circularity, cylindricity, parallelism and perpendicularity to planar datums, position of holes and pins RFS and MMC, DRF from planar datums; it must then be confirmed by a paid author in a reviewed pull request. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, no standards text anywhere.

## Open questions

- Family names for ISO (GEOTOL, DATUM, MATREQ, plus the shared SIZE and DPROF) are reserved here so `GDT.RES.ROWISO` can reuse the format.

## Children

_Leaf: none._
