---
id: CORE.BUS.UNIT
title: Authored literal normalisation to SI
level: 3
parent: CORE.BUS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS-014]
covers: []
depends_on: [CORE.REG.DEF]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.BUS.UNIT: Authored literal normalisation to SI

## Purpose

Converts a value typed by a person or script with a unit (for example `10.1 mm`) into the kernel's SI f64 with exactly one rounding, while keeping the literal exactly as entered for the journal. This makes an operator's input replay bit-exactly on every platform.

## Scope

**In:** parsing decimal literals with units, exact rational conversion using the quantity catalogue, one correctly rounded conversion to f64, recording the literal as entered, the versioned algorithm ID.
**Out:** the quantity catalogue and its factors (`CORE.REG.DEF`); parsing units on each surface (`AUTO`); display formatting (`VIZ`, `RPT`).

## Requirements

- **REQ-CORE.BUS.UNIT-001** When a parameter carries an authored literal, the bus shall record its decimal digits and unit symbol exactly as entered in the command arguments.
- **REQ-CORE.BUS.UNIT-002** When a literal is normalised, the bus shall compute the exact rational product of the decimal value and the catalogue conversion factor and round it to the nearest f64, ties to even, exactly once.
- **REQ-CORE.BUS.UNIT-003** If the literal's unit is not an accepted unit of the parameter's quantity kind, then the bus shall return a typed `validation` error with reason `unit_mismatch`.
- **REQ-CORE.BUS.UNIT-004** If a literal has more than 64 significant digits or a decimal exponent outside the range -400 to 400, then the bus shall return a typed `validation` error with reason `literal_out_of_range`.
- **REQ-CORE.BUS.UNIT-005** The normalisation shall record the algorithm ID `core.unit.normalise@1.0.0` in the entry's algorithm list.
- **REQ-CORE.BUS.UNIT-006** The same literal and unit shall normalise to the same f64 bits on every supported platform.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/unit/literals.csv` (at least 500 rows: literal, unit, kind, expected IEEE 754 bits in hex, produced by an independent exact-rational reference script), when each row is normalised, then every result's bits equal the expected bits.
- **AC-2** Given `10.1 mm` for a length parameter, when normalised, then the recorded literal is `{"value": "10.1", "unit": "mm"}` and the SI value bits equal those of the correctly rounded f64 nearest to 0.0101.
- **AC-3** Given `0.1 in` for a length parameter, when normalised, then the result equals the correctly rounded value of 0.00254 exactly (one rounding, not two).
- **AC-4** (error path) Given `5 deg` for a length parameter, then the call fails with `validation` / `unit_mismatch` and journals nothing.
- **AC-5** (error path) Given a literal of 65 significant digits, then the call fails with `literal_out_of_range`.
- **AC-6** (replay) Given the journal of AC-2, when replayed in exact mode on another platform class in CI, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** none; a pipeline helper called during validation.
- **Data structures:** `AuthoredLiteral { value: DecimalString, unit: UnitSymbol }`; `Normalised { si: f64, literal: AuthoredLiteral, algorithm: AlgorithmId }`.
- **Algorithm:** parse the decimal into sign, integer significand and base-10 exponent; multiply by the factor held as an exact rational (numerator, denominator) from the catalogue; convert the rational to f64 with a correctly rounded big-integer division (round half to even). No floating-point arithmetic before the single rounding. Affine units (for example degrees Celsius) are out of scope for P0 and rejected by the catalogue.
- **Errors:** `unit_mismatch`, `literal_out_of_range`, `literal_syntax`.
- **Determinism:** D0; integer arithmetic only, then one IEEE rounding.
- **Performance budget:** < 5 µs per literal [unverified until measured].
- **Crate:** `fid-bus` (module `unit`).

## Test fixtures

`fixtures/core/bus/unit/literals.csv`, generated once by a reference script using exact rationals (for example Python `fractions`) and committed with the script; CC-BY-4.0. Rows include halfway cases, subnormal results and inch, micrometre and degree conversions.

## Build prompt

> In crate `fid-bus`, add module `unit` with `normalise(literal: &AuthoredLiteral, kind: &QuantityKind, catalogue: &QuantityCatalogue) -> Result<Normalised, BusError>`. Parse the decimal string (at most 64 significant digits, exponent -400..400) into an integer significand and exponent; look up the unit's exact rational factor for the kind (reject a unit not accepted for that kind with `unit_mismatch`); compute the exact rational product with a big-integer type from the allowlist and convert to f64 with a single round-half-to-even. Never use f64 arithmetic before that rounding. Keep the literal as entered and return algorithm ID `core.unit.normalise@1.0.0`. Tests: AC-1..AC-6 with `fixtures/core/bus/unit/literals.csv` and its generator script. Constraints: CONSTITUTION §4 (SI metres, one journaled rounding), §8 (no panic, no unsafe, bounded parsing, deny unwrap/expect).

## Open questions

- Affine units (temperature in degrees Celsius or Fahrenheit) need an offset and are deferred to a later version of the algorithm.

## Children

_Leaf: none._
