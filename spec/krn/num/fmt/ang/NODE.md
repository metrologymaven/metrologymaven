---
id: KRN.NUM.FMT.ANG
title: Angle formatters with a rational enclosure of pi
level: 4
parent: KRN.NUM.FMT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM.FMT-004, REQ-KRN.NUM.FMT-005, REQ-KRN.NUM.FMT-006]
covers: []
depends_on: [KRN.NUM.FMT.LEN, KRN.NUM.KDEC, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.FMT.ANG: Angle formatters with a rational enclosure of pi

## Purpose

Angles are held in radians, but drawings give angular limits in degrees. Converting radians to degrees multiplies by 180/pi, which is irrational, so no finite exact computation gives the digits. This leaf brackets pi between two fixed rationals, computes the degree value as an interval, and emits digits only when both ends of the interval round the same way. It decides the side of a degree limit the same way and returns a typed `undecidable_side` result instead of guessing. It covers radians, decimal degrees and degrees-minutes-seconds.

## Scope

**In:**
- `fmt.angle@1` with units rad (factor 1, through the LEN digit engine), deg (decimal degrees) and dms (degrees, minutes, seconds)
- A fixed ladder of rational enclosures of pi, tried from tightest-cheapest to tightest-dearest
- Interval rounding: digits emitted only when both enclosure ends give the same rounded result
- Side decision against limits in degrees (or radians) and the side-preserving rule, reusing LEN's rule and flags
- DMS rendering with carry (59.9995 seconds rounding to 60 carries into minutes and degrees)

**Out** (and which node owns it instead):
- The shared types, digit engine and length formatters (`KRN.NUM.FMT.LEN`)
- Registry data, dispatch and the version golden diff (`KRN.NUM.FMT.REG`)
- Degree, minute and second symbols and locale typography (`I18N.LOCALE` supplies the rules; `RPT` and `VIZ` apply them)
- Angular tolerance interpretation (`GDT`)

## Requirements

- **REQ-KRN.NUM.FMT.ANG-001** The module shall hold a ladder of at least three rational enclosures [p_lo, p_hi] of pi with p_hi - p_lo at most 10^-50, 10^-200 and 10^-1000 [unverified sizes], built from a published decimal expansion and checked in tests against an independent source.
- **REQ-KRN.NUM.FMT.ANG-002** When an angle in radians is formatted in deg or dms, the formatter shall compute the interval of x * 180 / pi over the enclosure, and shall emit digits only when both interval ends round half-even to the same output.
- **REQ-KRN.NUM.FMT.ANG-003** If both ends round differently at every enclosure in the ladder, then the formatter shall return a typed `undecidable_rounding` error and no string.
- **REQ-KRN.NUM.FMT.ANG-004** When a limit in degrees is given, the formatter shall decide whether x lies above, below or on the limit by comparing x * 180 with L * pi over the enclosure ladder, and if no enclosure decides it, shall return a typed `undecidable_side` result rather than guess.
- **REQ-KRN.NUM.FMT.ANG-005** When the exact angle lies outside a limit, the formatter shall apply the same side-preserving rule as `fmt.length@1` (add digits up to 17 significant digits until the output is strictly on the violated side, else set `tie_at_max_precision`).
- **REQ-KRN.NUM.FMT.ANG-006** When formatting in dms, the formatter shall round the seconds field once, carry into minutes and degrees, and output minutes and whole seconds in the range 0 to 59.
- **REQ-KRN.NUM.FMT.ANG-007** Every successful call shall return a `Reported` record whose `exact_conversion` names the pi enclosure used (ladder index), and the formatter shall return a typed error for NaN or infinite input.
- **REQ-KRN.NUM.FMT.ANG-008** The formatter shall produce identical outputs on x86-64, ARM64 and wasm32, using integer arithmetic only after decomposing the f64 bits.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/fmt/ang-golden-v1` (20 000 cases: random and boundary radians, units rad, deg and dms, precisions 0..8 decimals, expected strings from an offline high-precision computation with at least 2000 digits of pi), when `fmt::ang::format` runs, then every string is identical.
- **AC-2** Given x = the f64 nearest to pi/4 and unit deg with 6 decimals, then the output is `45.000000`; given x = the f64 nearest pi/2 in dms with 1 decimal on seconds, the output is `90:00:00.0`.
- **AC-3** (carry) Given an angle whose seconds field rounds to 60.0, then the dms output carries to the next minute and shows `00.0` seconds.
- **AC-4** (side) Given upper limit `30.000` degrees and x = next f64 above the f64 nearest pi/6, with 3 decimals, then either the output is strictly above `30.000` under exact comparison or, if the enclosure cannot decide x against 30 degrees, the result is `undecidable_side`; it is never `30.000` with the side unknown.
- **AC-5** (enclosure check) Given the ladder, then each p_lo < p_hi, each enclosure contains the next tighter one, and the 50-digit enclosure matches an independently computed pi in the test.
- **AC-6** (error path) Given NaN or infinity, then the call returns `NonFinite` and no `Reported`.
- **AC-7** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then a SHA-256 digest of all outputs is identical.

## Design

- **Registry command(s):** none; library API `fid_num::fmt::ang::format(x_rad, &FormatSpec) -> Result<Reported, NumError>`.
- **Data structures:** `PiEnclosure { lo: &'static str, hi: &'static str, digits: u16 }` as decimal digit strings parsed once into a fixed-capacity big integer; the canonical dms output is `[-]D:MM:SS[.s...]` (ASCII; symbols are added by the locale render).
- **Algorithm:**
  1. Decompose x into a dyadic rational.
  2. For each enclosure in the ladder, compute the degree interval [x*180/p_hi, x*180/p_lo] (ends swapped for negative x) as rationals.
  3. Round both ends with LEN's digit engine; accept when equal.
  4. Side tests compare x*180 with L*p_lo and L*p_hi.
  5. DMS splits the accepted degree value into integer degrees, minutes and seconds with exact rational remainders before the single rounding of seconds.
  The big integer needs a capacity for about 1000-digit operands; parameterise the KDEC big integer by capacity or use a second fixed size.
- **Errors:** `FmtError::{NonFinite, UndecidableRounding, PrecisionOutOfRange, LimitsInverted}`; `undecidable_side` is a result variant `SideOutcome::Undecidable` carried with the output, not a panic.
- **Determinism:** D0; integer arithmetic only.
- **Performance budget:** < 20 microseconds per call with the first enclosure, < 2 ms when the tightest is needed [unverified until measured].
- **Crate:** `fid-num` (module `fmt::ang`).

## Test fixtures

`fixtures/num/fmt/ang-golden-v1`: generated by a committed offline script using arbitrary-precision arithmetic (for example Python `mpmath` at 2000 digits), CC-BY-4.0. The pi digit strings in source are a mathematical constant, not reference-dataset content.

## Build prompt

> In crate `fid-num`, add module `fmt::ang` implementing `fmt.angle@1` for units rad, deg and dms, reusing the types and digit engine from `fmt::len` and the big integer from `kdec`. Embed a ladder of three rational enclosures of pi (gaps at most 1e-50, 1e-200, 1e-1000) as decimal digit strings with a test that checks nesting and the first 50 digits against an independent computation. For deg/dms: decompose the f64 bits; for each enclosure compute the interval of x*180/pi exactly as rationals; round both ends half-even with the LEN digit engine; accept when equal; if never equal return `UndecidableRounding`. Side test against a degree limit L: compare x*180 with L*p_lo and L*p_hi; if no enclosure decides, return `SideOutcome::Undecidable` (never guess). If outside a limit, apply LEN's side-preserving rule (add digits up to 17 significant, else `tie_at_max_precision`). DMS: split exactly, round seconds once, carry to minutes and degrees, output `[-]D:MM:SS[.s]` in ASCII. Record the enclosure index in `exact_conversion`. Tests: `fixtures/num/fmt/ang-golden-v1` (20 000 cases from 2000-digit offline arithmetic), AC-2..AC-6 examples, identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1 and §8): no FMA, no float transcendentals, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O, no recursion; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-7 of spec node KRN.NUM.FMT.ANG.

## Open questions

- Phase P0 so that `KRN.NUM.FMT.REG` (P0) can register and dispatch it; Track A transcription may also carry angular characteristics. LEN remains the first to build.
- Whether `undecidable_rounding` can occur at all for a non-zero dyadic x is a number-theory question; the ladder sizes are chosen so it should never occur in practice [unverified].
- The KNP-1 dataset-constant ban (`KRN.NUM.LINT.DATA`) must not flag the pi digit strings; pi is listed in the RDL denylist's common-constant exclusions only if `UNC.RDL.LINT` adds it. Confirm with UNC.RDL.

## Children

_Leaf: none._
