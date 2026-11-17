---
id: RPT.STATS.TREND.CONTRACT
title: ChartSeries contract and versioning
level: 4
parent: RPT.STATS.TREND
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.TREND-004, REQ-RPT.STATS.TREND-005]
covers: []
depends_on: [CORE.JRN.CANON, GEO.FRAME.UNITS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.STATS.TREND.CONTRACT: ChartSeries contract and versioning

## Purpose

Defines `ChartSeries`, the one data shape every chart source hands to report chart blocks and to the viewer: trend and histogram series from this module, control charts from `RPT.STATS.SPC`, vector group plots from LVM. It carries points, axes with units, reference lines, markers and provenance, and it is versioned so a consumer can refuse a shape it does not understand. Owning the shape in one place keeps chart blocks free of computation.

## Scope

**In:**
- The `ChartSeries` Rust type, its JSON Schema `chart-series.schema.json` (contract major 1) and serde round-trip
- Series kinds, reference-line kinds and marker kinds for contract 1.0
- Provenance fields
- The compatibility check `check_supported` and error `report.chart_series_unsupported`

**Out:**
- Building trend and histogram series (`RPT.STATS.TREND.BUILD`, `RPT.STATS.TREND.HIST`)
- Building control-chart series (`RPT.STATS.SPC.CHART`)
- Laying out and drawing charts (`RPT.MODEL.BLOCKS`, VIZ)

## Requirements

- **REQ-RPT.STATS.TREND.CONTRACT-001** A `ChartSeries` shall carry a contract version (major and minor), a series kind, an x axis and a y axis each with a quantity kind and unit or `ORDINAL`, an ordered point list, reference lines, per-point markers and a provenance block.
- **REQ-RPT.STATS.TREND.CONTRACT-002** The provenance block shall hold the data-set digest, the algorithm ID and version, the kernel version and, when frozen control limits were used, the frozen-limits record ID; a series without a data-set digest or algorithm ID shall fail validation with `report.chart_series_invalid`.
- **REQ-RPT.STATS.TREND.CONTRACT-003** Contract 1.0 shall define the series kinds `TREND`, `HISTOGRAM`, `CONTROL_XBAR`, `CONTROL_R`, `CONTROL_S`, `CONTROL_I`, `CONTROL_MR` and `VECTOR_GROUP`, the reference-line kinds `NOMINAL`, `LSL`, `USL`, `CENTRE`, `LCL` and `UCL`, and the marker kinds `OUT_OF_TOLERANCE`, `INDETERMINATE` and `RULE_VIOLATION`.
- **REQ-RPT.STATS.TREND.CONTRACT-004** A `RULE_VIOLATION` marker shall name the run-rule set ID, its version and the rule ID.
- **REQ-RPT.STATS.TREND.CONTRACT-005** When `check_supported` is called with a series whose contract major differs from the consumer's supported major, or whose kind the consumer does not support, it shall return error `report.chart_series_unsupported` naming the version and kind.
- **REQ-RPT.STATS.TREND.CONTRACT-006** A minor version shall only add optional fields, and a consumer of the same major shall accept and ignore unknown optional fields.
- **REQ-RPT.STATS.TREND.CONTRACT-007** Serialising a `ChartSeries` to RFC 8785 canonical JSON and parsing it back shall give an equal value, with f64 values kept bit for bit.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/chart_series/v1_examples/` (one valid series per kind), when each is validated against `chart-series.schema.json` and round-tripped through canonical JSON, then all validate and round-trip bit for bit.
- **AC-2** (version) Given `fixtures/rpt/stats/chart_series/v2_trend.json` (major 2), when `check_supported(series, major = 1)` runs, then it returns `report.chart_series_unsupported` naming version 2.
- **AC-3** (forward-compatible minor) Given `v1_examples/trend_minor9.json` (major 1, minor 9, one unknown optional field), when parsed by a 1.0 consumer, then it is accepted and the unknown field is ignored.
- **AC-4** (error path) Given a series with no data-set digest, when validated, then it fails with `report.chart_series_invalid` naming `provenance.dataset_digest`.

## Design

- **Registry commands:** none (types and a validation function). `side_effect: R0` describes the read-only validation helper.
- **Data structures:**
  - `ChartSeries { contract: ContractVersion { major: 1, minor: 0 }, kind: SeriesKind, x: Axis, y: Axis, points: Vec<Point>, ref_lines: Vec<RefLine>, provenance: Provenance }`
  - `Axis { kind: AxisKind::{Ordinal, Quantity{quantity, unit}}, label_key: String }`
  - `Point { x: f64, y: f64, x_label: Option<String>, record: Option<RecordId>, piece: Option<PieceId>, markers: Vec<Marker> }`; histograms use one point per bin with `x` the lower edge, `y` the count and an optional `x_upper`.
  - `RefLine { kind: RefKind, value: f64, unit }`; `Marker::{OutOfTolerance, Indeterminate, RuleViolation{set_id, set_version, rule_id}}`
  - `Provenance { dataset_digest, algorithm: AlgorithmId, kernel_version, frozen_limits: Option<RecordId>, binning_rule: Option<RuleId> }`
- **Schema:** JSON Schema 2020-12 file `chart-series.schema.json`, published as an artifact with the crate so that `RPT.MODEL.BLOCKS`, LVM and VIZ can validate without linking the crate.
- **Algorithm:** none beyond validation; units come from the unit catalogue (GEO.FRAME.UNITS); canonical JSON from `fid-canon`.
- **Errors:** `report.chart_series_unsupported{major, kind}`, `report.chart_series_invalid{field}`.
- **Determinism:** D0.
- **Performance budget:** validate a 100 000-point series in under 20 ms [unverified until measured].
- **Crate:** `fid-chart-series` (L2 RPT types crate, no I/O).

## Test fixtures

- `fixtures/rpt/stats/chart_series/v1_examples/`, `v2_trend.json` and `trend_minor9.json`: hand-written JSON examples, CC-BY-4.0.

## Build prompt

> Create crate `fid-chart-series` (layer L2, RPT; types only, no I/O; depends on `fid-canon` and the unit catalogue) defining the `ChartSeries` contract, major 1, minor 0.
>
> **Type:** `ChartSeries { contract {major, minor}, kind, x: Axis, y: Axis, points: Vec<Point>, ref_lines: Vec<RefLine>, provenance }`. Kinds: `TREND, HISTOGRAM, CONTROL_XBAR, CONTROL_R, CONTROL_S, CONTROL_I, CONTROL_MR, VECTOR_GROUP`. Reference lines: `NOMINAL, LSL, USL, CENTRE, LCL, UCL`. Markers: `OUT_OF_TOLERANCE, INDETERMINATE, RULE_VIOLATION{set_id, set_version, rule_id}`. Provenance: data-set digest and algorithm ID (both required), kernel version, optional frozen-limits record ID, optional binning rule ID. Axis is `ORDINAL` or a quantity with unit. Points carry x, y, optional label, record ID, piece ID and markers.
>
> **Also:** export `chart-series.schema.json` (JSON Schema 2020-12); `validate(&ChartSeries) -> Result<(), ChartSeriesError>` failing with `report.chart_series_invalid{field}`; `check_supported(&ChartSeries, supported_major, supported_kinds) -> Result<(), ChartSeriesError>` failing with `report.chart_series_unsupported{major, kind}`. Same-major consumers ignore unknown optional fields; minor versions only add optional fields. Canonical JSON round-trip must keep f64 bits.
>
> **Tests:** AC-1 to AC-4 above with fixtures under `fixtures/rpt/stats/chart_series/`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`, bounded parsing). Spec: `spec/rpt/stats/trend/contract/NODE.md`.

## Open questions

- `VECTOR_GROUP` is reserved for LVM's vector group plot; LVM may need extra optional fields (vector components) in a later minor version.
- Consumers (`RPT.MODEL.BLOCKS`, VIZ) should declare `uses_artifacts: [RPT.STATS.TREND.CONTRACT]` for the schema file, or depend on the crate; that edit is theirs to make.

## Children

_Leaf: none._
