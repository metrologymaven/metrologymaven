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
built_in: [fid-num]
satisfies: [REQ-KRN.NUM.FMT-004, REQ-KRN.NUM.FMT-005, REQ-KRN.NUM.FMT-006]
covers: []
depends_on: [KRN.NUM.FMT.LEN, KRN.NUM.KDEC, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **Pinned details** (every output string depends on them; any implementation must match string for string; the oracle `fixtures/num/fmt/make_ang_fixtures.py` models exactly these):
  - **Formatter and units:** `fmt.angle@1` (`FormatterId::ANGLE_V1`). `rad` is the `fmt.scalar@1` rule of LEN with factor 1 and limits in radians (`exact_conversion` rad -> rad, `"1"`, no enclosure). `deg` prints w = |x| * 180 / pi with `Decimals(0..=20)` or `SigDigits(1..=17)`. `dms` rounds w = |x| * 648000 / pi (seconds) once at `Decimals(p)`, p being the decimals of the seconds field (0..=20); `SigDigits` is rejected for dms. Limits for deg and dms are decimal degrees (`kdec::Decimal`). There is no gon unit: it is not in this node's scope.
  - **Ladder:** `PI_LADDER` = three rungs lo = floor(pi * 10^d) / 10^d, hi = lo + 10^-d, d = 50, 200, 1000, stored as `3.<d decimals>` strings and parsed into the big integer per call. lo < pi < hi strictly; each rung lies inside the one before. At rung i the value interval is [|x| * c / hi_i, |x| * c / lo_i] (c = 180 or 648000), two exact rationals; the sign is applied after rounding, so the ends need no swap.
  - **Decisions:** a decision is taken at the first rung, from the current one upward, at which both ends agree; the current rung then moves there (nesting makes the result the one the exact value gives). The decisions of a call, in order: (1) the requested half-even rounding (equal `(q, p)` at both ends; with significant digits each end uses its own decimal exponent, sound because rounding is monotone and equal outputs have equal `(q, p)`); (2) the side of the upper limit, then of the lower limit only if the upper one is not violated; (3) only with a violated limit and decimals, the decimal exponent k of w (k = 0 for zero); (4) each rounding of LEN's side loop. If (1), (3) or (4) is decided at no rung, the call returns `UndecidableRounding` and no string.
  - **Side test:** exact when x and L differ in sign or either is zero (only x = 0 with L = 0 is on a limit); else |x| * 180 >= |L| * hi_i is above, <= |L| * lo_i is below. If no rung decides, the current rung becomes the last, `side = SideOutcome::Undecidable`, the lower limit is not tested, and the output is the requested rounding with `digits_added = 0` and no tie flag; it says nothing about the side.
  - **Side-preserving rule:** LEN's loop and flags, unchanged (same code): one digit at a time, zero outputs skipped at once (skipped steps are not decisions), cap 17 significant digits of w (k + 1 + p >= 17 for decimals, w in degrees for deg and in seconds for dms), directed rounding toward the violated side at the cap with `tie_at_max_precision = true`. The output is compared with the limit exactly and without pi: degrees against L, or seconds against 3600 * L.
  - **dms rendering:** with q the rounded count of 10^-p seconds, D = q div (3600 * 10^p), M = (q mod 3600 * 10^p) div (60 * 10^p), S the rest; printed `[-]D:MM:SS[.s...]`, D unpadded, MM and the integer part of SS two digits, p decimals. Rounding the total once is the single rounding of the seconds field followed by the carry (59.96 s at one decimal gives `11:00:00.0` from 10:59:59.96).
  - **Sign:** `-` only when x is negative and q is not zero, as LEN.
  - **Record:** `Reported` gains `side: SideOutcome` (`NoLimits`, `Within`, `AboveUpper`, `BelowLower`, `Undecidable`; LEN sets the first four) and `Conversion` gains `pi_enclosure: Option<u8>`: for deg and dms the final current rung, which is the smallest ladder index at which every decision of the call is decided (factor string `180/pi`); `None` for exact factors.
  - **Error order:** `UnitNotSupported` (a formatter other than `fmt.angle@1`, op `fmt`; a unit other than rad, deg, dms, op `fmt.angle@1`), `PrecisionOutOfRange`, `LimitsInverted`, `NonFinite`, then `UndecidableRounding` (detail `undecidable_rounding`).
  - **Capacity:** the big integer is LEN's `kdec` type made generic over its limb count; this module uses 192 limbs (6144 bits). The largest intermediates are the side test (|L| = 2^128 * 10^400 against a subnormal at the 1000-digit rung, 5854 bits) and roundings of the largest double in seconds (below 4600 bits), so `big_integer_capacity` cannot occur.
  - **Rung use in practice:** the 50-digit rung decides almost every call; values above about 10^30 degrees printed to whole units need the 200- or 1000-digit rung (`f64::MAX` at 0 decimals needs 1000). An undecidable outcome needs an angle within about 10^-1000 relative of a rounding boundary or a limit and has not been observed; the tests exercise both with a coarse ladder of 3, 6 and 10 decimals.

## Test fixtures

`fixtures/num/fmt/ang-golden-v1.txt` (with `ang-ac-v1.txt`, the coarse-ladder cases `ang-ladder-v1.txt` and pi to 2100 decimals in `ang-pi-v1.txt`): generated by the committed offline script `fixtures/num/fmt/make_ang_fixtures.py` with the standard library only (pi from the Chudnovsky series in integers, exact rationals, and a second path through the `decimal` module at 2300 digits), CC-BY-4.0. The script asserts the side-safety property on every case. The pi digit strings in source are a mathematical constant, not reference-dataset content.

## Build prompt

> In crate `fid-num`, add module `fmt::ang` implementing `fmt.angle@1` for units rad, deg and dms, reusing the types and digit engine from `fmt::len` and the big integer from `kdec`. Embed a ladder of three rational enclosures of pi (gaps at most 1e-50, 1e-200, 1e-1000) as decimal digit strings with a test that checks nesting and the first 50 digits against an independent computation. For deg/dms: decompose the f64 bits; for each enclosure compute the interval of x*180/pi exactly as rationals; round both ends half-even with the LEN digit engine; accept when equal; if never equal return `UndecidableRounding`. Side test against a degree limit L: compare x*180 with L*p_lo and L*p_hi; if no enclosure decides, return `SideOutcome::Undecidable` (never guess). If outside a limit, apply LEN's side-preserving rule (add digits up to 17 significant, else `tie_at_max_precision`). DMS: split exactly, round seconds once, carry to minutes and degrees, output `[-]D:MM:SS[.s]` in ASCII. Record the enclosure index in `exact_conversion`. Tests: `fixtures/num/fmt/ang-golden-v1` (20 000 cases from 2000-digit offline arithmetic), AC-2..AC-6 examples, identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1 and §8): no FMA, no float transcendentals, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O, no recursion; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-7 of spec node KRN.NUM.FMT.ANG.

## Open questions

- Phase P0 so that `KRN.NUM.FMT.REG` (P0) can register and dispatch it; Track A transcription may also carry angular characteristics. LEN remains the first to build.
- Whether `undecidable_rounding` can occur at all for a non-zero dyadic x is a number-theory question; the ladder sizes are chosen so it should never occur in practice [unverified].
- The KNP-1 dataset-constant ban (`KRN.NUM.LINT.DATA`) must not flag the pi digit strings; pi is listed in the RDL denylist's common-constant exclusions only if `UNC.RDL.LINT` adds it. Confirm with UNC.RDL.

## Children

_Leaf: none._
