---
id: SCAN.CMP.DEV.FIELD
title: DeviationField element, builder and canonical digest
level: 4
parent: SCAN.CMP.DEV
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-cmp]
built_pr: 202
satisfies: [REQ-SCAN.CMP.DEV-003, REQ-SCAN.CMP.DEV-005, REQ-SCAN.CMP.DEV-006]
covers: []
depends_on: [GEO.TYPES.DIGEST, GEO.TYPES.ENV, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.CMP.DEV.FIELD: DeviationField element, builder and canonical digest

## Purpose

The `DeviationField` element that stores a comparison: one value and status per actual point, plus the method, parameters and digests that make it reproducible. It can be built only through a crate-private `FieldBuilder` that the CPU deviation path hands out, so no other code (and in particular no GPU preview) can create one. Its canonical digest depends only on the values in original point order, so chunked and in-memory runs give the same digest.

## Scope

**In:**
- `DeviationField` element type: `values: Vec<f64>` (0.0 where no value), `status: Vec<DevStatus>` (`VALID`, `NO_MATCH`, `EXCLUDED`), `provenance`
- `FieldProvenance`: method, max search distance (m), angle cutoff (rad), alignment ID and digest, actual digest, nominal digest and kind, actual and nominal selection digests, algorithm ID, kernel version
- `FieldBuilder`: crate-private constructor, `set(index, DevResult)` in any order, `finish()` checking every index was set exactly once
- Canonical byte layout and SHA-256 digest

**Out** (and which node owns it instead):
- Computing values (owned by `SCAN.CMP.DEV.PRIM`)
- The command (owned by `SCAN.CMP.DEV.SURFCAD`)
- The preview type (owned by `SCAN.CMP.GPUPV.PVFIELD`)

## Requirements

- **REQ-SCAN.CMP.DEV.FIELD-001** A `DeviationField` shall record method, maximum search distance in metres, angle cutoff in radians, alignment ID, and the digests of the actual, nominal and alignment versions, and shall hold exactly one status per actual point.
- **REQ-SCAN.CMP.DEV.FIELD-002** `FieldBuilder` shall be constructible only inside the `fid-scan-cmp` crate's kernel deviation path, and `DeviationField` shall have no public constructor and no conversion from any other buffer type, enforced by compile-fail tests.
- **REQ-SCAN.CMP.DEV.FIELD-003** If `finish()` is called while any index is unset or was set twice, then it shall fail with `scan.cmp.field_incomplete` naming the first offending index.
- **REQ-SCAN.CMP.DEV.FIELD-004** A point with status other than `VALID` shall store value 0.0 and shall be excluded from every statistic the field exposes.
- **REQ-SCAN.CMP.DEV.FIELD-005** The field digest shall be SHA-256 over magic `FIDDEV`, layout version, canonical JSON of the provenance, the point count, then per point in original index order the status byte and the value as little-endian f64, so that it is independent of fill order and thread count.

## Acceptance criteria

- **AC-1** Given a builder for 5 points filled in order 4, 2, 0, 1, 3 and another filled 0..4 with the same results, then both fields have the same digest.
- **AC-2** (error path) Given a builder with index 3 unset, `finish()` fails with `scan.cmp.field_incomplete { index: 3 }`.
- **AC-3** Given a compile-fail test constructing `DeviationField` or `FieldBuilder` from outside the crate, compilation fails.
- **AC-4** Given a field with values and a `NO_MATCH` point, the canonical JSON of provenance and the digest match the golden file `fixtures/scan/cmp/field_golden.json`.

## Design

- **API** (`fid-scan-cmp::field`): `pub struct DeviationField { .. }` with read accessors (`len`, `value(i) -> Option<f64>`, `status(i)`, `provenance()`, `digest()`, `valid_indexes()`); `pub(crate) struct FieldBuilder`; `pub(crate) fn FieldBuilder::new(n, provenance)`; `DevStatus` as `u8` repr.
- **Data:** values and statuses as dense columns. As built, the element kind is not yet registered through GEO.TYPES.ENV (see *Element kind* below and Open questions), and the provenance is canonicalised with `fid-kci`'s kernel canonical JSON (KRN.MAN.KCI), not CORE.JRN.CANON (see *Provenance JSON* below).
- **Errors:** `scan.cmp.field_incomplete`, `scan.cmp.non_finite` (NaN or infinity in a `VALID` value).
- **Determinism:** D0.
- **Crate:** `fid-scan-cmp` (kernel tier, KNP-1). The crate must not depend on any GPU crate; the dependency allowlist check enforces it.
- **Pinned at build (value-affecting choices; the Python reference `fixtures/scan/cmp/make_field_golden.py` implements exactly these):**
  - *Layout version 1*, integers little-endian: `"FIDDEV"` | `0x01` | `prov_len` u64 | provenance canonical JSON (`prov_len` bytes) | `n` u64 | `n` × (`status` u8 | `value` f64 IEEE 754 bits). The provenance length prefix makes the layout unambiguous without parsing the JSON. Digest: SHA-256 (`fid-kci`'s portable FIPS 180-4 code) over the layout, written in the `sha256:` wire form (`fid-types::Digest`).
  - *Status bytes:* `VALID` 1, `NO_MATCH` 2, `EXCLUDED` 3 (0 is never a status).
  - *Values:* a `VALID` value must be finite (else `scan.cmp.non_finite` at `set`, nothing recorded); `VALID` −0.0 is stored as +0.0; every other value keeps its bits; non-`VALID` points store +0.0. Values are metres, signed as the primitive returns them (positive material-outside, SCAN.CMP.DEV.PRIM); the field never rescales or re-signs.
  - *Provenance JSON:* the kernel canonical JSON of KRN.MAN.KCI (RFC 8785 restricted to strings, arrays and objects; every number a string), because CORE.JRN.CANON's writer (`fid-canon`) is in the model layer, which a kernel crate may not depend on. Members: `actual_digest`, `actual_selection` (omitted when absent), `algorithm_id` (KRN.MAN.CARD form), `alignment_digest`, `alignment_id`, `kernel_version`, `max_angle_rad`, `max_distance_m`, `method` (`normal` / `shortest`), `nominal_digest`, `nominal_kind` (GEO.TYPES.ENV wire name), `nominal_selection` (omitted when absent). The two numbers are the ECMAScript `Number::toString` of the f64 (RFC 8785 §3.2.2.3 form) inside a JSON string; −0.0 gives `"0"`.
  - *Provenance domain* (`scan.cmp.invalid_parameter`, the code SCAN.CMP.DEV.PRIM uses): `max_distance_m` finite and > 0; `max_angle_rad` in [0, `f64::consts::PI`] (−0.0 accepted); `alignment_id` and `kernel_version` not empty.
  - *Builder:* `set(index, DevResult)` with `index ≥ n` fails `scan.cmp.field_index_out_of_range` and records nothing; `finish()` names the lowest index that is unset or set more than once (`scan.cmp.field_incomplete { index, kind: unset | set_twice }`).
  - *Empty input:* `n = 0` is valid; the field is empty, its digest covers the header and count 0, and its statistics have no values.
  - *Bounded allocation:* the builder allocates exactly three dense columns, 10 bytes per point, with fallible reservation (`scan.cmp.allocation_failed { needed }`, `needed = 10·n` or `u64::MAX` on overflow) instead of aborting; hashing streams through a fixed 9 KiB block (1 024 points), independent of `n`.
  - *Statistics* (`stats()`), over `VALID` points only (REQ-004): counts per status; totalOrder min and max (`fid-num::f`); mean by KRN.NUM.RED `mean` (fixed block-and-tree Neumaier sum, one division). An overflowing sum fails `scan.cmp.non_finite` with no index.
  - *Element kind:* the wire name is `deviation_field`. GEO.TYPES.ENV's `ElementKind` is a closed geometric set in `fid-geo-types`, which cannot depend on this crate; adding the kind there is left to GEO.TYPES.REG.

## Test fixtures

`fixtures/scan/cmp/field_golden.json` (golden cases) and `fixtures/scan/cmp/field_numbers.txt` (f64 number strings), both generated by the independent stdlib Python reference `fixtures/scan/cmp/make_field_golden.py` (`--check` in CI), CC0.

## Build prompt

> In the Rust crate `fid-scan-cmp`, module `field`, implement `DeviationField` (dense `values: Vec<f64>`, `status: Vec<DevStatus>` with `VALID | NO_MATCH | EXCLUDED`, `FieldProvenance { method, max_distance_m, max_angle_rad, alignment_id, alignment_digest, actual_digest, nominal_digest, nominal_kind, actual_selection?, nominal_selection?, algorithm_id, kernel_version }`) with no public constructor, and a `pub(crate)` `FieldBuilder` with `set(index, DevResult)` and `finish()` that fails `scan.cmp.field_incomplete` on unset or doubly-set indexes. Non-`VALID` points store 0.0 and are excluded from statistics; NaN or infinite values fail `scan.cmp.non_finite`. Digest: SHA-256 over `FIDDEV`, layout version, RFC 8785 provenance, count, then per point status byte and value (LE f64) in original index order. Register the element kind with GEO.TYPES.ENV.
>
> Constraints: CONSTITUTION C2 (no GPU dependency in this crate), §8 (no panic/unwrap/unsafe), KNP-1. Tests: AC-1..AC-4, including compile-fail tests (trybuild).

## Open questions

- The field has its own content digest (layout magic `FIDDEV`) but no `ElementKind`: GEO.TYPES.ENV's closed kind set lives in `fid-geo-types`, which cannot depend on this crate. Until GEO.TYPES.REG adds a `deviation_field` kind (or an equivalent registration path), no `GeometryElement` envelope can carry or identify a `DeviationField`.

## Children

_Leaf: none._
