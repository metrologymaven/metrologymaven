---
id: UNC.INST.SCHEMA
title: Instrument model data format, import, binding and range check
level: 3
parent: UNC.INST
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-001, REQ-UNC.INST-004]
covers: [SA.UNC.01]
depends_on: [UNC.MODEL.DIST, HAL.IDL, CORE]
side_effect: W2
determinism: D0
cli: [fid unc import-instrument-model, fid unc bind-instrument-model, fid unc show-instrument-model]
mcp: [unc_import_instrument_model, unc_bind_instrument_model, unc_show_instrument_model]
journal: [unc.import_instrument_model@1, unc.bind_instrument_model@1]
updated: 2026-11-24
---

# UNC.INST.SCHEMA: Instrument model data format, import, binding and range check

## Purpose

Defines `InstrumentModel` as versioned data (never code): named parameters with units and distributions, a valid operating range, a source citation, a model kind (specification-derived, IEMPS, per-observation), and a content digest. It provides the import, show and bind commands, and the range check that flags any observation outside the model's validity instead of extrapolating. Every other INST child produces or consumes this format.

## Scope

**In:**
- `instrument-model/v1` JSON schema and canonical digest
- `unc.import_instrument_model@1` (W2), `unc.show_instrument_model@1` (R0), `unc.bind_instrument_model@1` (W2) to a device instance (HAL device identity) or to a project default
- `check_range(model, observation_context) -> RangeVerdict` used by producers before they evaluate a model

**Out:**
- Term-to-contributor mappings for MPE models (owned by `UNC.INST.MPE`)
- Per-observation covariance formulas (owned by `UNC.INST.PORT`)
- IEMPS acceptance-record verification (owned by `UNC.INST.IEMPS`)
- The device descriptor format `instrument-profile/v1` (owned by `HAL.IDL`); this node references it by ID and digest

## Requirements

- **REQ-UNC.INST.SCHEMA-001** Every instrument model shall carry `id` (`im:<name>@<semver>`), kind (`spec_derived | iemps | observation`), device class, named parameters with SI units and optional distributions, a valid operating range (per named axis such as range, angle, temperature), a source citation and a SHA-256 digest of its RFC 8785 canonical form.
- **REQ-UNC.INST.SCHEMA-002** If an imported model fails schema validation, has a parameter without a unit, has an empty or inverted range, or lacks a source citation, then import shall fail with a typed error naming the field and commit nothing.
- **REQ-UNC.INST.SCHEMA-003** If an imported model reuses an existing `id` with a different digest, then import shall fail with `version_conflict`.
- **REQ-UNC.INST.SCHEMA-004** When `unc.bind_instrument_model@1` binds a model to a device instance, the binding shall record the model digest and the HAL device identity and descriptor digest.
- **REQ-UNC.INST.SCHEMA-005** If an observation context lies outside any declared range axis of the bound model, then `check_range` shall return `out_of_model_range` naming the axis and the violated bound, and producers shall attach that flag to the result rather than evaluate the model by extrapolation.
- **REQ-UNC.INST.SCHEMA-006** The source citation shall be stored as an untrusted string and never interpreted (CONSTITUTION C6).

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/tracker_example.json` (a synthetic tracker observation model with range axis 0.5..40 m), when `fid unc import-instrument-model --file <handle> --json` runs, then the model is stored with the digest in `fixtures/unc/inst/digests.txt` and one `unc.import_instrument_model@1` event is journaled.
- **AC-2** (error path) Given `fixtures/unc/inst/invalid/no_unit.json`, `inverted_range.json` and `no_source.json`, when imported, then each fails with the named field and journals nothing.
- **AC-3** (bind) Given simulated device `sim-tracker-1` from the HAL simulator descriptor, when `fid unc bind-instrument-model --model im:tracker-example@1.0.0 --device sim-tracker-1` runs, then the binding records the model digest and the device descriptor digest.
- **AC-4** (range) Given an observation at 52 m, when `check_range` runs, then it returns `out_of_model_range` with axis `range` and bound 40 m.
- **AC-5** (version) Given a second file with the same `id` and different content, then import fails with `version_conflict`.
- **AC-6** (replay) Given the journal from AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.import_instrument_model@1` (W2; `file: ImportHandle` from an approved import root, never a raw path), `unc.show_instrument_model@1` (R0; `id`), `unc.bind_instrument_model@1` (W2; `model: ModelRef`, `target: DeviceId | Project`).
- **Data structures:** `InstrumentModel { id, semver, kind, device_class, params: Vec<Param{name, unit, value, dist: Option<Distribution>}>, range: Vec<RangeAxis{axis, min, max, unit}>, source: UntrustedString, digest }`; `InstrumentBinding { model, target, descriptor_digest }`; `RangeVerdict = InRange | OutOfModelRange{axis, bound} | NotChecked{axis, reason: not_observed | unit_mismatch}` with `must_flag()`, true for every verdict but `InRange`.
- **Algorithm:** bounded JSON parse (max 1 MB, max depth 32), schema validation, unit conversion to SI by single correctly rounded parse, canonicalize and digest, store. Range check compares each axis with closed bounds.
  - **Units are SI or SI-prefixed only** (m, km, mm, um, nm, rad, mrad, urad, K, mK, s, ms, us, 1, ppm, um/m): a prefix is a shift of the decimal exponent, so the conversion stays one correctly rounded parse. Degrees and degrees Celsius need a second rounding or an offset and are refused (`schema_invalid`).
  - **Empty range:** an axis with `min == max` is empty and, like `min > max`, is `invalid_range{axis}`. A model with no range axes at all is `schema_invalid{range}`.
  - **Unchecked axes (D-045):** a declared axis the observation context does not hold, or holds in another SI unit, is `NotChecked`. The model's validity was not established, so producers flag it exactly like `OutOfModelRange` (`must_flag`) and never present the result as in range. An axis out of range is reported before an unchecked one.
  - `unc.show_instrument_model@1` is R0 and is not journaled (CONSTITUTION §3, D-031).
- **Errors:** `schema_invalid{field}`, `missing_unit{param}`, `invalid_range{axis}`, `missing_source`, `version_conflict`, `device_not_found`.
- **Determinism:** D0.
- **Performance budget:** import of a 1 MB model in under 50 ms [unverified until measured].
- **Crate:** `fid-unc-inst`.

## Test fixtures

`fixtures/unc/inst/`: synthetic example models (tracker, arm, scanner, CMM) with made-up parameter values clearly labelled as examples, invalid variants, and digests; CC-BY-4.0. The HAL simulator descriptor for `sim-tracker-1` comes from `HAL.SIM`.

## Build prompt

> In crate `fid-unc-inst`, define the `instrument-model/v1` JSON schema and `InstrumentModel` type: `id` `im:<name>@<semver>`, kind (`spec_derived | iemps | observation`), device class, parameters with SI units and optional distributions (use `Distribution` from `fid-kunc` module `dist`), range axes with min/max/unit, an untrusted source citation (never interpreted, C6), and a SHA-256 digest over RFC 8785 canonical JSON.
> Implement registry commands `unc.import_instrument_model@1` (W2; CLI `fid unc import-instrument-model`; takes an import-root handle, never a raw path; bounded parse ≤ 1 MB, depth ≤ 32), `unc.show_instrument_model@1` (R0) and `unc.bind_instrument_model@1` (W2; binds to a HAL device identity, recording the `instrument-profile/v1` descriptor digest, or to a project default). Each is journaled with its command ID and result digest.
> Errors: `schema_invalid{field}`, `missing_unit{param}`, `invalid_range{axis}`, `missing_source`, `version_conflict` (same id, different digest), `device_not_found`; failures commit nothing. Provide `check_range(model, ctx) -> InRange | OutOfModelRange{axis, bound} | NotChecked{axis, reason}`; producers must flag every verdict but `InRange` (`must_flag`), never extrapolate.
> Tests with `fixtures/unc/inst/`: import digest matches; each invalid file fails with its field; bind to simulated `sim-tracker-1`; 52 m observation flagged against the 40 m bound; version conflict; exact replay IDENTICAL. Support `--json` and `--dry-run`. No panics, no `unwrap`.

## Open questions

- Whether the range check should also cover environmental axes reported by HAL (temperature, warm-up state) depends on the provenance stamp fields frozen by `HAL.IDL`.

## Children

_Leaf: none._
