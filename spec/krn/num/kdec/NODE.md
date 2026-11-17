---
id: KRN.NUM.KDEC
title: Exact decimal type and comparator (kdec)
level: 3
parent: KRN.NUM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM-001, REQ-KRN.NUM-007, REQ-KRN.NUM-011]
covers: []
depends_on: [KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.NUM.KDEC: Exact decimal type and comparator (kdec)

## Purpose

Limits on drawings are decimals (10.050 mm), while kernel values are binary64. Converting the limit to f64 and comparing can give the wrong side for values within one ulp of a limit. `kdec` keeps limits as exact decimals and orders an f64 (an exact dyadic rational) against a decimal by exact integer arithmetic. It is one of the two kernel pieces that Track A transcription reports need in P0 (with `KRN.NUM.FMT`), and the same comparator decides manual entries.

## Scope

**In:** the `Decimal` type (sign, integer coefficient, base-10 exponent); a bounded strict parser; exact unit shifts by powers of ten; exact multiplication by a decimal unit factor (for example 0.0254); the exact comparator f64 vs `Decimal` and `Decimal` vs `Decimal`; correctly rounded `Decimal` -> f64 conversion; a small internal unsigned big-integer used only by this module and `KRN.NUM.FMT`.
**Out:** formatting values for display (`KRN.NUM.FMT`); decision rules, guard bands and conformance probability (`UNC`); journaling a unit conversion as `unit.convert` (`CORE` command layer); angle limits, which need an enclosure of pi (`KRN.NUM.FMT`).

## Requirements

- **REQ-KRN.NUM.KDEC-001** `Decimal::parse` shall accept an optional sign, digits, an optional fraction and an optional exponent, at most 40 significant digits and exponents within plus or minus 400, and shall return a typed error for any other input without allocating more than a fixed bound.
- **REQ-KRN.NUM.KDEC-002** `kdec::cmp(x: f64, d: &Decimal)` shall return the exact ordering of the real number x and the real number d, and shall return `NumError::NotANumber` or `Infinite` for non-finite x.
- **REQ-KRN.NUM.KDEC-003** `cmp` shall return `Equal` only when x and d denote exactly the same real number (for example 0.5 and 0.50, never 0.1 and the nearest f64 to 0.1).
- **REQ-KRN.NUM.KDEC-004** `Decimal::shift10(k)` and `Decimal::mul(&Decimal)` shall be exact, so a limit authored in mm or inch converts to metres with no rounding.
- **REQ-KRN.NUM.KDEC-005** `Decimal::to_f64` shall return the correctly rounded (round-half-even) binary64 value of the decimal, identical on every target.
- **REQ-KRN.NUM.KDEC-006** Every operation shall run in bounded time and memory for inputs within the parse limits, with no recursion.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/kdec-boundary-v1` (20 000 cases: for each of 5 000 decimal limits D in mm and in metres, the f64 values nearest(D), next_up, next_down and nearest(D) +/- 10 ulp, with the expected ordering computed offline by exact rational arithmetic), when `cmp` runs, then every ordering matches.
- **AC-2** Given limit "10.050" mm shifted to metres and x = nearest f64 to 0.0100500004, when `cmp` runs, then the result is `Greater`; given x = nearest f64 to 0.01005, then the result is `Greater` or `Less` (whichever is exact) and never `Equal`.
- **AC-3** Given 10 000 decimal strings from the same fixture, when `to_f64` runs, then each result is bit-equal to the offline correctly rounded value and to `core` `str::parse::<f64>` of the same string.
- **AC-4** (error path) Given "1e999", "--1", "0x10", "", "NaN" and a 41-significant-digit string, when `Decimal::parse` runs, then each returns a typed parse error.
- **AC-5** Given the same fixture on x86-64, ARM64 and wasm32 under wasmtime, then all outputs are identical.

## Design

- **Registry command(s):** none; module `fid_num::kdec`.
- **Data structures:** `pub struct Decimal { neg: bool, coeff: u128, exp10: i16 }` normalised (no trailing zeros in `coeff` unless zero); internal `struct Big { limbs: [u32; N] , len: usize }` with N fixed (sized for 2^1100 times 10^440 [unverified bound, derive exactly in the implementation]).
- **Algorithm:** decompose x into (sign, mantissa m, binary exponent e) from its bits. Compare m * 2^e with c * 10^k by moving all factors to non-negative exponents on both sides (multiply by 2^max(0,-e) and 10^max(0,-k)) and comparing the two big integers. Signs and zero are handled first. `to_f64` uses `core::str::parse::<f64>` on the canonical string (correctly rounded in core [unverified since which Rust version; pin the toolchain]), cross-checked in tests by an exact big-integer rounding.
- **Errors:** `KdecError::{Syntax, TooManyDigits, ExponentRange}` wrapped in `NumError::Domain`; non-finite inputs as `NumError`.
- **Determinism:** D0; integer arithmetic only.
- **Performance budget:** one `cmp` < 2 microseconds worst case [unverified until measured].
- **Crate:** `fid-num` (module `kdec`).

## Test fixtures

`fixtures/num/kdec-boundary-v1`: generated by a committed offline script using exact rational arithmetic (for example Python `fractions`), CC-BY-4.0.

## Build prompt

> In crate `fid-num`, create module `kdec`. Implement `pub struct Decimal { neg: bool, coeff: u128, exp10: i16 }` with a strict bounded parser (sign, digits, fraction, exponent; max 40 significant digits, |exp| <= 400; typed errors, no panics), `shift10(k)`, exact `mul(&Decimal)` (error if the coefficient overflows u128), `to_f64()` (correctly rounded; use `core` float parsing on the canonical string), and `pub fn cmp(x: f64, d: &Decimal) -> Result<core::cmp::Ordering, NumError>` that compares exactly: decompose the f64 bits into sign, integer mantissa and binary exponent, then compare m*2^e and c*10^k using a fixed-capacity, stack-allocated, limb-based unsigned big integer (no recursion, no heap growth beyond a fixed bound). Also `cmp_dec(&Decimal, &Decimal)`. Non-finite x returns `NotANumber`/`Infinite` from module `err`. Tests: `fixtures/num/kdec-boundary-v1` (20 000 orderings computed offline by exact rationals), the 10.050 mm boundary example, parse error cases, and identical results on x86-64, ARM64, wasm32 under wasmtime. KNP-1: no FMA, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.NUM.KDEC.

## Open questions

- The big-integer bound must cover the full binary64 exponent range against the full decimal exponent range of the parser; derive the limb count exactly when building.

## Children

_Leaf: none._
