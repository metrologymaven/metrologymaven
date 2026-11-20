---
id: IO.RES.ILABEL
title: Interpretation label guard for every results export
level: 3
parent: IO.RES
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.RES-013, REQ-IO.RES-014]
covers: []
depends_on: [IO.RES.CSV.MAP, IO.RES.AQDEF.KSET]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# IO.RES.ILABEL: Interpretation label guard for every results export

## Purpose

D-045 says a GD&T result that rests on an interpretation rule without expert sign-off carries `interpretation: provisional` in every export, and is never presented as settled. A results file is the easiest place for that label to fall off silently: a CSV column map without the column, or a format such as AQDEF with no field for it. This leaf is the one guard every results exporter calls before writing. It tells the exporter where the label goes, or refuses the export.

## Scope

**In:**
- `LabelCapability` of an export target: `Field(name)`, `Extension(hook)` or `None`
- Capabilities of the built-in targets:
  - CSV and table exports: `Field`, when the column map binds a column to `interpretation`;
  - AQDEF: `None`, until the K-field subset names a field for it;
  - QIF Results: `Extension`, through QIF's sanctioned extension hook;
  - PiWeb and report-file envelopes: as each exporter declares.
- `ensure_interpretation_label(records, capability)`
- The values each exporter writes: `interpretation` and, where the target can hold it, the `interpretation_rules` list

**Out:**
- Writing the files (owned by the exporters: `IO.RES.CSV.OUT`, `IO.RES.AQDEF.OUT`, `IO.RES.QIFR.ROUT`, `IO.RES.PIWEB`, `IO.RES.REPORT`)
- Computing the label (owned by `GDT.CHAR.INTERP`; this leaf reads the ResultRecord field)

## Requirements

- **REQ-IO.RES.ILABEL-001** If any record to be exported has `interpretation` `provisional` and the target's capability is `None`, then `ensure_interpretation_label` shall fail with `interpretation_label_unsupported` naming the first such record and the target, and the exporter shall write nothing.
- **REQ-IO.RES.ILABEL-002** When the capability is `Field` or `Extension`, the guard shall return the label value of every record (`provisional` or `signed_off`), and the exporter shall write it for every record, signed-off ones included.
- **REQ-IO.RES.ILABEL-003** A record set with no provisional record shall pass the guard for every capability, `None` included.
- **REQ-IO.RES.ILABEL-004** A CSV column map's capability shall be `Field` only if one of its columns is bound to the ResultRecord field `interpretation`. A column bound to any other field, or a constant, shall not count.

## Acceptance criteria

- **AC-1** (CSV, error path) Given records from `fixtures/gdt/interp/position_mmc_mixed/` (provisional) and the column map `fixtures/io/res/csv/maps/basic.toml`, which has no `interpretation` column, when `fid io export-results --format csv --map basic.toml --json` runs, then it fails with `interpretation_label_unsupported` naming the record, and no file is written.
- **AC-2** (CSV) Given the same records and the map `basic_interp.toml`, which binds column `Interpretation` to `interpretation`, then the export succeeds and every row's `Interpretation` cell is `provisional`.
- **AC-3** (AQDEF, error path) Given the same provisional records, when `fid io export-results --format aqdef --json` runs with the published K-field subset, then it fails with `interpretation_label_unsupported` and no DFQ file is written.
- **AC-4** (AQDEF) Given records from `fixtures/gdt/interp/flatness_signed/` (signed off only), then the AQDEF export succeeds.
- **AC-5** Given a column map with a constant column `Interpretation = signed_off`, then its capability is `None`, and AC-1's records fail as in AC-1.

## Design

- **Registry command(s):** none; a library used by every results exporter.
- **Data structures:** `LabelCapability { Field(FieldName), Extension(HookId), None }`; `LabelPlan { per_record: Vec<(RecordId, Interpretation)> }`.
- **Algorithm:** scan the records. If any is provisional and the capability is `None`, fail. Otherwise return the plan.
- **Errors:** `interpretation_label_unsupported{record, target}`.
- **Determinism:** D0.
- **Crate:** `fid-io-res` (module `ilabel`).

## Test fixtures

The `fixtures/gdt/interp/` records shared with `GDT.CHAR.INTERP`, and new column maps `basic_interp.toml` and `const_interp.toml` under `fixtures/io/res/csv/maps/` (CC0).

## Build prompt

> In crate `fid-io-res`, add module `ilabel` with `LabelCapability { Field, Extension, None }` and `ensure_interpretation_label(records, capability) -> Result<LabelPlan, ExportError>`:
> - Fail with `interpretation_label_unsupported{record, target}` when any record's `interpretation` is `provisional` and the capability is `None`.
> - Otherwise return each record's label.
> - A CSV column map has capability `Field` only if a column is bound to the record field `interpretation` (constants do not count).
> - AQDEF has `None` under the published K-field subset.
>
> Call the guard from the CSV and AQDEF exporters before any byte is written, and write the label column when present. Tests: AC-1..AC-5. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`.

## Open questions

- `IO.RES.CSV.OUT` is already built. Calling the guard from it changes that crate's behaviour, so the build of this leaf includes that call and its tests; `IO.RES.CSV.OUT`'s own acceptance criteria are unchanged.
- An AQDEF K-field that could carry the label is for `IO.RES.AQDEF.KSET` to propose [unverified whether a suitable free field exists].

## Children

_Leaf: none._
