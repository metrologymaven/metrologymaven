---
id: KRN.NUM.FMT.LEN
title: Length and dimensionless formatters with the side-preserving rule
level: 4
parent: KRN.NUM.FMT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-num]
built_pr: 200
satisfies: [REQ-KRN.NUM.FMT-002, REQ-KRN.NUM.FMT-003, REQ-KRN.NUM.FMT-005, REQ-KRN.NUM.FMT-006]
covers: []
depends_on: [KRN.NUM.KDEC, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
---

# KRN.NUM.FMT.LEN: Length and dimensionless formatters with the side-preserving rule

## Purpose

The formatter core that Track A transcription reports need in P0. It turns an SI f64 (metres, or a dimensionless ratio) into a canonical decimal string in a display unit by exact rational arithmetic with a single half-even rounding, and, when limits are given, never prints a value lying outside a limit as equal to it or on the wrong side of it. It also defines the shared formatter types (`FormatSpec`, `Reported`, `FormatterId`, `Conversion`) and the digit engine that `KRN.NUM.FMT.ANG` reuses.

## Scope

**In:**
- Shared types: `FormatterId { kind, major }`, `FormatSpec`, `Precision { Decimals(u8) | SigDigits(u8) }`, `Limits { lower: Option<Decimal>, upper: Option<Decimal> }`, `Conversion`, `Reported`
- The digit engine: exact rational value -> canonical decimal string at a requested precision, one round-half-even step
- `fmt.length@1`: display units m, mm, um (micrometre), in, with exact factors 1, 10^3, 10^6 and 1/0.0254
- `fmt.scalar@1`: dimensionless values (unit `1`), factor 1
- The side-preserving rule against limits, the `digits_added` count and the `tie_at_max_precision` flag
- Canonical output form: ASCII digits, `.` and leading `-` only; no grouping, no exponent

**Out** (and which node owns it instead):
- Angle formatters and the pi enclosure (`KRN.NUM.FMT.ANG`)
- Formatter registry data, `format` dispatch, hex rendering and the cross-version golden diff (`KRN.NUM.FMT.REG`)
- Exact decimal type, comparator and big integer (`KRN.NUM.KDEC`)
- Locale separators, grouping, minus sign and digit shapes (`I18N.LOCALE` supplies the rules and transforms; `RPT` and `VIZ` apply them)
- Choosing decimals per characteristic and the visible tie marker (`RPT.MODEL.FMT`); deciding conformance (`UNC`)

## Requirements

- **REQ-KRN.NUM.FMT.LEN-001** When a finite value x in metres is formatted with `fmt.length@1` in unit u, the formatter shall compute the digits from the exact rational x / f(u), with f(m) = 1, f(mm) = 10^-3, f(um) = 10^-6 and f(in) = 0.0254 exactly, and shall round half-even exactly once.
- **REQ-KRN.NUM.FMT.LEN-002** When a finite dimensionless value is formatted with `fmt.scalar@1`, the formatter shall compute the digits from the exact dyadic value with one half-even rounding.
- **REQ-KRN.NUM.FMT.LEN-003** The formatter shall accept a precision of 0 to 20 decimals or 1 to 17 significant digits, and shall return a typed `precision_out_of_range` error otherwise.
- **REQ-KRN.NUM.FMT.LEN-004** When limits are given and the exact value lies strictly above the upper limit or strictly below the lower limit (decided by `kdec`), the formatter shall add one digit at a time, up to 17 significant digits, until the rounded output lies strictly on the violated side of that limit under exact comparison.
- **REQ-KRN.NUM.FMT.LEN-005** If the rounded output still does not lie strictly on the violated side at 17 significant digits, then the formatter shall round the last digit at that precision toward the violated side (toward +infinity for an upper limit, toward -infinity for a lower limit) instead of half-even, and shall return that output with `tie_at_max_precision = true`, so that no printed value lies on the wrong side of, or equal to, a violated limit.
- **REQ-KRN.NUM.FMT.LEN-006** When the exact value lies inside or on the limits, the formatter shall output the value at the requested precision without adding digits, even if the output equals a limit.
- **REQ-KRN.NUM.FMT.LEN-007** Every successful call shall return `Reported { value, unit, exact_conversion, formatter, digits_added, tie_at_max_precision }`, where `exact_conversion` names the source unit, target unit and the exact factor as a decimal or ratio string.
- **REQ-KRN.NUM.FMT.LEN-008** The canonical output shall contain only ASCII digits, at most one `.` and an optional leading `-`, and shall print a minus sign only when the printed digits are not all zero.
- **REQ-KRN.NUM.FMT.LEN-009** If the input is NaN or infinite, or a limit pair has lower greater than upper, then the formatter shall return a typed error and no string.
- **REQ-KRN.NUM.FMT.LEN-010** The formatter shall produce identical outputs on x86-64, ARM64 and wasm32, using integer arithmetic only after decomposing the f64 bits.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/fmt/len-golden-v1` (50 000 cases: random and boundary f64 values in metres, each unit, precisions 0..20 decimals and 1..17 significant digits, expected strings computed offline with exact rationals), when `fmt::len::format` runs, then every string is identical.
- **AC-2** Given x = the f64 nearest to 0.0254 * 1.0005 metres and unit `in` with 3 decimals, then the output is `1.000` or `1.001` exactly as the offline exact half-even result says; and given x = the f64 nearest 0.01 m in `mm` with 3 decimals, the output is `10.000`.
- **AC-3** (side rule) Given upper limit Decimal `0.00005` (metres, that is 0.050 mm) and x = next f64 above 5e-5 m, unit `mm`, 3 decimals, then the output is not `0.050`, is strictly greater than `0.050` under `kdec`, and `digits_added >= 1`.
- **AC-4** (wrong-side guard) Given upper limit Decimal `0.0000505` (metres), x = the f64 nearest 5.06e-5 m, unit `mm` and 2 decimals, then the output is never `0.05`; it gains digits until strictly above `0.0505`.
- **AC-5** (conforming) Given upper limit Decimal `0.00005` (metres) and x = the f64 nearest 4.99996e-5 m, unit `mm` with 3 decimals, then the output is `0.050` with `digits_added = 0` (REQ-006).
- **AC-6** (error path) Given NaN, +inf, 25 decimals, 18 significant digits, or lower > upper, then each call returns its typed error and no `Reported`.
- **AC-7** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then a SHA-256 digest of all outputs is identical.
- **AC-8** Given x = -1e-12 m, unit mm, 3 decimals and no limits, then the output is `0.000` (no minus sign); with lower limit `0` it is `-0.000000001` or the shortest strictly negative rounding.
- **AC-9** (directed at the cap) Given upper limit Decimal `0.506108` (metres), x = the f64 nearest 0.506108 m (which lies above it), unit `in` and 15 significant digits, where half-even at 17 digits would print `19.925511811023622` (below the limit, 19.92551181102362204...), then the output is strictly greater than the limit under `kdec` after exact conversion back to metres and `tie_at_max_precision = true`; likewise the mirrored case (lower limit `-0.506108`, x = -0.506108) is strictly below its limit.

## Design

- **Registry command(s):** none; library API in `fid_num::fmt` (`len` module). Callers reach it through `fmt::format` in `KRN.NUM.FMT.REG` or directly.
- **Data structures:** `FormatterId { kind: &'static str, major: u16 }` displayed as `fmt.<kind>@<major>`; `FormatSpec { formatter: FormatterId, unit: Unit, precision: Precision, limits: Option<Limits> }` with limits in metres (or the dimensionless value) as exact `kdec::Decimal`; `Conversion { from: Unit, to: Unit, factor: &'static str, pi_enclosure: Option<u8> }` (`"1"`, `"1000"`, `"1000000"`, `"1/0.0254"`; `pi_enclosure` is `None` here and names the pi ladder index in `KRN.NUM.FMT.ANG`); `Reported { value: String, unit: Unit, exact_conversion: Conversion, formatter: FormatterId, digits_added: u16, tie_at_max_precision: bool, side: SideOutcome }`, where `SideOutcome` is `NoLimits`, `Within`, `AboveUpper`, `BelowLower` or (`KRN.NUM.FMT.ANG` only) `Undecidable`; this leaf sets `NoLimits` when no limit value is given, `AboveUpper`/`BelowLower` for the violated limit, else `Within`. Both fields were added by `KRN.NUM.FMT.ANG` (#213) and change no output string.
- **Algorithm:**
  1. Decompose x into sign, integer mantissa m and binary exponent e.
  2. Form the exact rational N / D for x / f(u) (for inches, multiply by 10^4 and divide by 254).
  3. Scale by 10^p (p = decimals, or derived from significant digits and the decimal exponent of N / D), divide with the KDEC big integer, and round half-even on the exact remainder.
  4. Render the canonical string.
  5. With limits, compare x with each limit by `kdec::cmp`. If outside, compare the rounded output (converted back to metres exactly, as a Decimal times the unit factor) with the violated limit by `kdec::cmp_dec`, and repeat step 3 with one more digit until strictly on the violated side or 17 significant digits are reached.
  The big integer division routine is added to the KDEC internal big integer under this leaf (KDEC states that type is shared only with FMT).
- **Pinned details** (every output string depends on them; any implementation must match string for string; the oracle `fixtures/num/fmt/make_len_fixtures.py` models exactly these):
  - **Exact value:** x = (-1)^s * m * 2^e from the bits; the display value is v = N / D with N = m * 2^max(e,0) * 10^a and D = 2^max(-e,0) * b, where (a, b) = (0, 1) for m and `1`, (3, 1) for mm, (6, 1) for um and (4, 254) for in.
  - **Decimals(p):** q = round_half_even(|v| * 10^p), printed with exactly p digits after the point; p = 0 prints no point.
  - **SigDigits(n):** k = floor(log10 |v|) decided exactly; p = n - 1 - k. If rounding carries into the next decade (q = 10^n), the output is 10^(n-1) at p - 1 decimals, so it keeps exactly n significant digits (9.9996 at 3 digits is `10.0`). A negative p prints q followed by -p zeros, never an exponent (1.23e6 at 3 digits is `1230000`). Zero (either sign) uses k = 0: `0` at 1 digit, `0.00` at 3.
  - **Canonical string:** ASCII digits, at most one `.`, at least one digit before the point (`0.050`, never `.050`), no grouping or exponent; `-` only when x is negative and q is not zero, so -0.0 and negative values that round to zero print unsigned.
  - **Violated limit:** the upper one if `kdec::cmp(x, upper)` is Greater, otherwise the lower one if `kdec::cmp(x, lower)` is Less. Neither: the requested output, `digits_added = 0`, even if it equals a limit (REQ-006).
  - **Side loop:** at the current precision, if the output converted back exactly (q * 10^-p * f(u)) is strictly beyond the violated limit, stop. Otherwise, if at the cap, replace the output by the value rounded at the same precision toward the violated side (signed ceiling of v * 10^p for an upper limit, signed floor for a lower one; with significant digits a carry into the next decade is renormalised as above) and stop with `tie_at_max_precision = true`. That output is at least as far beyond the limit as x itself, so it is always strictly beyond (REQ-005); otherwise add one digit (p + 1 decimals, or n + 1 significant digits), count it and repeat. Cap: n = 17 for significant digits; k + 1 + p >= 17 for decimals (k = 0 for zero). A request that already shows 17 or more significant digits is never extended (and is rounded toward the violated side at once if half-even does not show it). Decimals added by the rule may exceed the 20 a caller may request (AC-8 needs 9 from 3; a subnormal needs over 300).
  - **Shortcut:** while the output is zero it stays zero for every p < -k - 1, so those steps are taken at once; the result and `digits_added` equal the one-digit-at-a-time loop.
  - **Comparison of the output with the limit:** exact, with the same result as `kdec::cmp_dec` of the output as a Decimal (tested on every fixture case where the output fits a Decimal). It runs on the KDEC big integer because an output coefficient can exceed the u128 of `Decimal` (for example 20 decimals of 1e30 m).
  - **Flag name:** `tie_at_max_precision` keeps its name (`KRN.NUM.FMT.ANG`, `KRN.NUM.FMT` and `RPT.MODEL.FMT` refer to it). It now means "half-even at the cap did not show the violated side, so the last digit was rounded toward it"; the value is never a tie with the limit.
  - **`digits_added` is `u16`:** the side loop can add more than 255 digits (a subnormal value at 0 decimals against a violated limit), which a `u8` cannot count.
  - **Error order:** `UnitNotSupported` (unknown formatter, a unit other than m/mm/um/in for `fmt.length@1`, other than `1` for `fmt.scalar@1`), then `PrecisionOutOfRange` (decimals > 20, significant digits 0 or > 17), then `LimitsInverted` (lower > upper; equal limits are allowed), then `NonFinite` (NaN or infinity). The `op` of the `NumError::Domain` is the formatter ID (`fmt.length@1`, `fmt.scalar@1`), or `fmt` for an unknown formatter.
- **Errors:** `FmtError::{NonFinite, PrecisionOutOfRange, LimitsInverted, UnitNotSupported}` wrapped in `NumError::Domain`, plus `FmtError::Capacity` (`big_integer_capacity`) for an exhausted big integer, which cannot occur within these bounds (largest intermediate below 1400 of 2560 bits) and exists so that an impossible case is an error, never a wrong string.
- **Determinism:** D0; integer arithmetic only.
- **Performance budget:** one format call < 5 microseconds, including up to 17 digits of side refinement [unverified until measured]. Subnormal inputs at the extremes (very fine decimals, or many side-rule steps) work on numbers of over 1000 bits and can take up to about 340 big-integer divisions; they will likely exceed this budget.
- **Crate:** `fid-num` (module `fmt::len`, shared types in `fmt::types`).

## Test fixtures

`fixtures/num/fmt/len-golden-v1.txt` (with the named AC examples in `len-ac-v1.txt`): generated by the committed offline script `fixtures/num/fmt/make_len_fixtures.py` using exact rational arithmetic (for example Python `fractions`), CC-BY-4.0. It also serves as the frozen corpus that `KRN.NUM.FMT.REG` diffs between versions.

## Build prompt

> In crate `fid-num`, add module `fmt` with submodules `types` and `len`. In `types`, define `FormatterId { kind, major }` (display `fmt.<kind>@<major>`), `Unit { M, Mm, Um, In, One, Rad, Deg, Dms }`, `Precision { Decimals(u8), SigDigits(u8) }`, `Limits { lower: Option<kdec::Decimal>, upper: Option<kdec::Decimal> }` (SI values), `FormatSpec`, `Conversion { from, to, factor: &'static str }` and `Reported { value: String, unit, exact_conversion, formatter, digits_added: u16, tie_at_max_precision: bool }`. In `len`, implement `fmt.length@1` (units m, mm, um, in; exact factors 1, 1e3, 1e6, 1/0.0254) and `fmt.scalar@1` (unit `1`). Algorithm: decompose the f64 bits; form the exact rational value in the display unit; scale by 10^p; divide with the fixed-capacity big integer from `kdec` (add a bounded long-division routine there, no recursion, no heap growth beyond a fixed bound); round half-even once on the exact remainder; render ASCII digits, `.`, leading `-` only when the digits are not all zero. Precision: 0..20 decimals or 1..17 significant digits, else `PrecisionOutOfRange`. Side-preserving rule: if `kdec::cmp` says x is strictly outside a limit, add one digit at a time (max 17 significant digits) until the rounded output, converted back exactly, is strictly on the violated side under `kdec::cmp_dec`; if not reached, round the 17-digit output toward the violated side instead of half-even and return it with `tie_at_max_precision = true`. Inside or on the limits: no extra digits. Errors: `NonFinite`, `PrecisionOutOfRange`, `LimitsInverted`, `UnitNotSupported`, wrapped in `NumError`. Tests: `fixtures/num/fmt/len-golden-v1` (50 000 offline exact cases), the AC-2..AC-6, AC-8 and AC-9 examples, and an identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1 and §8): no FMA, no float arithmetic after bit decomposition, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-9 of spec node KRN.NUM.FMT.LEN.

## Open questions

- A conforming value that rounds to its limit (0.0499996 against 0.050) prints as the limit under REQ-006, which follows common practice [unverified]. REQ-RPT-007 forbids only the nonconforming case. Confirm with `RPT.MODEL.FMT` and `UNC.DEC` before freezing; a change would be `fmt.length@2`.
- `RPT.MODEL.FMT` REQ-003 has RPT request one digit at a time, while this leaf already adds digits inside the formatter when given limits. RPT can pass the violated limit and use `digits_added` and `tie_at_max_precision` (for its REQ-004 marker) instead of looping. Raise at the RPT.MODEL.FMT expansion.
- The formatter decides "outside" by exact comparison against the limits it is given. When a decision rule uses guard bands, the caller (RPT) passes the specification limit that the decision says was violated.
- A directed-rounding-with-marker variant would be a separate formatter kind, not a mode of this one.

## Children

_Leaf: none._
