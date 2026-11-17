---
id: KRN.NUM.FMT
title: Versioned kernel number formatters (fmt.*)
level: 3
parent: KRN.NUM
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM-001, REQ-KRN.NUM-007, REQ-KRN.NUM-012]
covers: []
depends_on: [KRN.NUM.KDEC, KRN.NUM.ERR]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.FMT: Versioned kernel number formatters (fmt.*)

## Purpose

A rounded number in a report is a reported value, so the kernel owns how values are printed. Each formatter `fmt.<kind>@<major>` turns an SI f64 into a decimal string in a display unit, records the exact unit conversion it used, and never prints a nonconforming value as equal to a limit. Reports, the viewer legend, flags and Track A transcription all call these formatters, so displayed numbers and recorded decisions always agree.

## Scope

**In:**
- Formatter identity: `fmt.<kind>@<major>` IDs, a formatter registry served as data, and the rule that any change to output strings is a new major
- Length (m, mm, micrometre, inch) and dimensionless formatters with exact decimal unit factors
- Angle formatters (rad, decimal degrees, degrees-minutes-seconds) using a rigorous rational enclosure of pi so the side of a limit is decided exactly or reported as undecidable
- Rounding modes: round-half-even to a requested number of decimals or significant digits
- The side-preserving rule: when the rounded string would equal a limit that the exact value violates, print more digits (up to a fixed bound) until the string shows the side
- The `reported{value, unit, exact_conversion, formatter}` record that the ResultRecord carries
- IEEE-754 hex rendering of f64 values for hashed forms

**Out** (and which node owns it instead):
- The exact comparator and decimal type (`KRN.NUM.KDEC`)
- Deciding conformance (`UNC`), and which digits a report template shows (`RPT`)
- Legend binning of deviation colours (`VIZ`, `SCAN`)
- Locale rules and transforms for separators, grouping, minus sign and typography (`I18N.LOCALE` supplies them; `RPT` and `VIZ` apply them)

## Requirements

- **REQ-KRN.NUM.FMT-001** Each formatter shall have an ID `fmt.<kind>@<major>`, and any change to the string a formatter produces for any input shall require a new major.
- **REQ-KRN.NUM.FMT-002** When a length or dimensionless value is formatted, the formatter shall compute the displayed digits from the exact dyadic value divided by the exact decimal unit factor, rounding half-even only once.
- **REQ-KRN.NUM.FMT-003** When a formatter is given limits and the exact value lies outside them, the formatter shall never output a string equal to a limit; it shall add digits, up to 17 significant digits, until the string differs from the limit on the correct side.
- **REQ-KRN.NUM.FMT-004** When an angle is formatted against a limit in degrees, the formatter shall decide the side using a rational enclosure of pi, and if the enclosure cannot decide it shall return a typed `undecidable_side` result rather than guess.
- **REQ-KRN.NUM.FMT-005** Every formatted output shall carry the record `reported{value, unit, exact_conversion, formatter}`.
- **REQ-KRN.NUM.FMT-006** The formatters shall return a typed error for non-finite inputs, and shall produce identical strings on x86-64, ARM64 and wasm32.

## Interfaces

- **Offers:** `fid_num::fmt::{format, Reported, FormatterId, FormatSpec{unit, decimals|sig_digits, limits}}`; the formatter registry (IDs and versions) as data for `KRN.MAN` (manifest), `RPT`, `VIZ` and `AUTO`.
- **Consumes:** `KRN.NUM.KDEC` (Decimal, exact big-integer arithmetic), `KRN.NUM.ERR`.

## Decomposition hints

LEN (length and dimensionless formatters, exact factors, side-preserving rule; P0 first, it is all Track A needs), ANG (angle formatters with the pi enclosure and DMS), REG (formatter IDs, registry data, hex rendering, version-change test that diffs outputs on a frozen corpus).

## Open questions

- Whether a conforming value that rounds to its limit (0.0499996 against 0.050) may print as the limit. Common practice allows it [unverified]; REQ-RPT-007 only forbids the nonconforming case. Confirm with `RPT` and `UNC` before the LEN leaf.
- The concept doc also allows directed rounding with a marker instead of extra digits. This node chooses extra digits; a marker variant would be a separate formatter kind.
- **Applied at split (2026-11-17):** the Out line now names `I18N.LOCALE` as the supplier of locale rules and transforms. The conforming-rounds-to-limit question is carried into `KRN.NUM.FMT.LEN` (REQ-006 allows it, pending RPT and UNC confirmation). A nonconforming value that still ties at 17 significant digits is returned with `tie_at_max_precision`, which `RPT.MODEL.FMT` turns into its visible marker.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.NUM.FMT.LEN | Length and dimensionless formatters with the side-preserving rule | 002 003 005 006 | none |
| KRN.NUM.FMT.ANG | Angle formatters with a rational enclosure of pi | 004 005 006 | none |
| KRN.NUM.FMT.REG | Formatter registry, dispatch, hex rendering and version guard | 001 006 | none |
