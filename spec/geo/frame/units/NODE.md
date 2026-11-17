---
id: GEO.FRAME.UNITS
title: Authored quantities and unit conversion
level: 3
parent: GEO.FRAME
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.FRAME-007]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.FRAME.UNITS: Authored quantities and unit conversion

## Purpose

How lengths and angles cross the kernel boundary: an authored decimal keeps its text and unit, is converted into SI metres or radians with exactly one rounding, and is converted back to a display unit with exactly one rounding. The conversion record is a small value that the owning command puts into its journal entry.

## Scope

**In:**
- `LengthUnit`: m, mm, µm, nm, in, ft, thou (0.001 in)
- `AngleUnit`: rad, deg, arcmin, arcsec, gon
- `AuthoredQuantity { text, unit, si }` with parsing of plain decimals and exponents
- Correctly rounded conversion of an exact decimal times an exact rational factor (lengths)
- Angle conversion using a fixed 256-bit binary approximation of π, rounded once
- Display conversion: metres or radians to a decimal string in a target unit with a given number of digits, round-half-even on the exact binary value

**Out:**
- The `geo.convert_units@1` command and journal placement (owned by `GEO.FRAME.CMD`)
- Quantity kinds beyond length and angle (temperature, time; owned by the domains that need them)
- Locale-aware formatting (owned by `RPT` and `VIZ`)

## Requirements

- **REQ-GEO.FRAME.UNITS-001** When a length is authored, the system shall compute the f64 nearest to the exact value (decimal × unit factor), with ties to even, so the conversion is a single rounding.
- **REQ-GEO.FRAME.UNITS-002** The authored quantity shall keep the original decimal text and unit unchanged alongside the SI value.
- **REQ-GEO.FRAME.UNITS-003** When an angle is authored in degrees, arcminutes, arcseconds or gon, the system shall convert to radians by multiplying the exact decimal by a 256-bit approximation of π and rounding once to f64.
- **REQ-GEO.FRAME.UNITS-004** If the authored text is not a finite decimal, has more than 40 significant digits or overflows f64 after conversion, then parsing shall fail with a typed error.
- **REQ-GEO.FRAME.UNITS-005** When an SI value is formatted to a display unit with n fractional digits, the result shall be the exact binary value times the factor rounded once, half to even, to n digits.
- **REQ-GEO.FRAME.UNITS-006** The conversion record shall serialise as `{ text, unit, si_hex }`, with `si_hex` the IEEE 754 bits in hexadecimal, for inclusion in journal entries.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/frame/units_cases.json` (2 000 authored lengths and angles with expected f64 bits from an independent arbitrary-precision computation), when parsed and converted, then every result matches bit-for-bit.
- **AC-2** Given `"1" in`, when converted, then `si` is the f64 nearest 0.0254 and `text` is `"1"`; given `"0.1" mm`, `si` is the f64 nearest 1e-4 (not 0.1 × 0.001 rounded twice).
- **AC-3** (error path) Given texts `"1e400"`, `"NaN"`, `"1,5"` and a 41-digit decimal, when parsed, then each fails with its typed error.
- **AC-4** Given the f64 nearest 0.0254 formatted in inches with 6 digits, when formatted, then the result is `"1.000000"`; the fixture's 500 display cases match exactly.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `LengthUnit`, `AngleUnit`, `Unit`, `AuthoredQuantity { text: String, unit: Unit, si: f64 }`, `UnitsError { NotDecimal, TooManyDigits, Overflow, NonFinite }`.
- **Algorithm:** parse the decimal to an integer mantissa and base-10 exponent; multiply by the unit's exact rational factor (e.g. in = 254/10000 m); round the rational to f64 with a small fixed-width big-integer routine (Clinger-style correct rounding). Angles use a 256-bit constant for π/180 and similar, then one rounding [unverified that 256 bits always suffices for the 40-digit limit; the fixture checks].
- **Errors:** as above.
- **Determinism:** D0; integer arithmetic only.
- **Performance budget:** 1 µs per conversion [unverified].
- **Crate:** `fid-geo-frame` (kernel graph, KNP-1 applies; no float parsing from std is used for authored values).

## Test fixtures

- `fixtures/geo/frame/units_cases.json`: random decimals (seed 13) plus hand-picked hard cases (halfway values, subnormal results, 0.1 mm); expected values from Python `fractions` and `mpmath` at 300 digits. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-frame` (pure Rust, no I/O, KNP-1), add module `units`: `LengthUnit { M, Mm, Um, Nm, In, Ft, Thou }`, `AngleUnit { Rad, Deg, Arcmin, Arcsec, Gon }`, `AuthoredQuantity { text, unit, si }` with `parse(text, unit) -> Result<AuthoredQuantity, UnitsError>` and `format(si, unit, digits) -> String`. Parse the decimal exactly (≤ 40 significant digits) into mantissa and exponent, multiply by the unit's exact rational factor, and round once to the nearest f64 (ties to even) with a fixed-width big-integer routine; never use `str::parse::<f64>` followed by a multiply. Angles: multiply by a 256-bit π constant and round once. Formatting: exact binary value times factor, rounded once half-to-even to `digits` places. Serialise as `{text, unit, si_hex}`. Acceptance: AC-1 2 000 fixture cases bit-exact; AC-2 inch and 0.1 mm cases; AC-3 typed errors for overflow, NaN, comma decimal, 41 digits; AC-4 display cases exact. Constraints: CONSTITUTION §4 (single journaled rounding), KNP-1, §8 (no panic, no `unsafe`). Files: `crates/fid-geo-frame/src/units.rs`, `src/bigint.rs`, `tests/units.rs`, `fixtures/geo/frame/units_cases.json`, `tools/fixtures/geo_units_cases.py`.

## Open questions

- Whether other domains (UNC, RPT) should reuse this module for non-length quantities, which would move it to a shared crate; kept in GEO.FRAME for now as the parent scoped it.

## Children

_Leaf: none._
