---
id: KRN.NUM.LINT.DATA
title: Reference-dataset denylist matcher
level: 4
parent: KRN.NUM.LINT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-knp1-lint]
built_pr: 245
satisfies: [REQ-KRN.NUM.LINT-005, REQ-KRN.NUM.LINT-007]
covers: []
depends_on: [KRN.NUM.LINT.FIND, KRN.NUM.LINT.GRAPH]
uses_artifacts: [UNC.RDL.LINT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LINT.DATA: Reference-dataset denylist matcher

## Purpose

Fails the build when kernel source names a reference dataset or embeds one of its reference values, so code (including agent-written code) cannot be tuned to the answers it is validated against. The knowledge of what to ban comes from `rdl-denylist.json`, published by `UNC.RDL.LINT`; this leaf is the matcher.

## Scope

**In:**
- Reading and validating `rdl-denylist.json` (version, digest, `ids`, `files`, `values` with decimal strings and hex literals, `excluded`)
- Matching over every file of the kernel closure: string literals and byte-string literals containing a dataset ID or blob file name (`KNP1-DATA-ID`, `KNP1-DATA-FILE`); numeric literals with 12 or more significant digits whose exact decimal value equals a denylisted decimal rendering (`KNP1-DATA-VALUE`); integer literals passed to `f64::from_bits` or float literals whose bits equal a denylisted hex literal (`KNP1-DATA-HEX`)
- Literal normalisation: strip `_` separators and type suffixes, parse exactly (no float conversion) before comparing
- Recording the denylist digest in the report

**Out** (and which node owns it instead):
- Generating the denylist and its common-constant exclusions, and the canary crate (`UNC.RDL.LINT`)
- Other source rules (`KRN.NUM.LINT.SYN`)

## Requirements

- **REQ-KRN.NUM.LINT.DATA-001** If `rdl-denylist.json` is missing, fails its schema check or its digest does not match its content, then the check shall fail closed with `KNP1-DATA-DENYLIST`.
- **REQ-KRN.NUM.LINT.DATA-002** The check shall report `KNP1-DATA-ID` or `KNP1-DATA-FILE` for each string or byte-string literal in a closure file that contains a denylisted dataset ID or blob file name as a substring.
- **REQ-KRN.NUM.LINT.DATA-003** The check shall report `KNP1-DATA-VALUE` for each numeric literal with 12 or more significant digits whose exact decimal value, after normalisation, equals a denylisted decimal rendering.
- **REQ-KRN.NUM.LINT.DATA-004** The check shall report `KNP1-DATA-HEX` for each float literal or `from_bits` integer argument whose IEEE-754 bit pattern equals a denylisted hex literal, regardless of digit count.
- **REQ-KRN.NUM.LINT.DATA-005** Values in the denylist's `excluded` section shall never produce a finding.
- **REQ-KRN.NUM.LINT.DATA-006** When run on the canary crate published by `UNC.RDL.LINT` with its denylist, the check shall report the planted constant, and shall report nothing once the constant is removed.

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/lint/canary_crate/` and `fixtures/rdl/lint/expected_denylist.json` (published by `UNC.RDL.LINT`), when the check runs, then exactly one `KNP1-DATA-VALUE` or `KNP1-DATA-HEX` finding names the planted constant.
- **AC-2** Given `fixtures/knp1/data/forms/` with the same reference value written as `0.123_456_789_012`, `0.123456789012f64` and `f64::from_bits(0x...)`, then each form is reported.
- **AC-3** Given a literal `0.12345678` (8 significant digits) equal to an 8-digit rendering in the denylist, then no finding (below 12 digits) unless its bits match a hex entry.
- **AC-4** Given the string literal `"tracim-lsq-circle-007.json"` where that file name is denylisted, then `KNP1-DATA-FILE`.
- **AC-5** (error path) Given a denylist whose digest does not match, then the run fails with `KNP1-DATA-DENYLIST` and no report of success.

## Design

- **Registry command(s):** none; module `data` of tooling crate `fid-knp1-lint`.
- **Data structures:** `Denylist` loaded from JSON; lookup sets as sorted vectors with binary search (`BTreeSet` acceptable in tooling) of normalised decimal strings, hex bit patterns, IDs and file names.
- **Algorithm:** reuse the syntax parse from `syn_scan` (token walk over literals, including inside macros); normalise numeric literals to an exact canonical decimal (sign, digits, exponent) without float conversion; compute float-literal bits by correctly rounded parsing for the hex comparison; substring search for IDs and file names.
- **Errors:** `KNP1-DATA-DENYLIST` (fatal); findings as listed.
- **Determinism:** D0.
- **Performance budget:** 300 000 lines against 100 000 denylisted values < 5 s [unverified until measured].
- **Crate:** `fid-knp1-lint` (tooling layer).

### Build decisions (fid-knp1-lint `data`)

- **Denylist schema (version 1)**, as `UNC.RDL.LINT`'s Design pins it: an object with exactly `version` (the number 1), `ids` and `files` (unique non-empty strings), `values` (objects with exactly `decimal`, a non-empty list of decimal strings, and `hex`, `0x` plus 16 lowercase hex digits), `excluded` (decimal strings) and `digest`, `sha256:` plus the hex SHA-256 of the RFC 8785 canonical JSON of the object without `digest`. Duplicate keys, unknown keys, a wrong digest, and a missing or unreadable file are fatal `KNP1-DATA-DENYLIST`.
- **Token walk.** Literals come from the bounded lexer of `KRN.NUM.LINT.SYN` (macro bodies and invocations included), after CRLF is turned into LF as rustc does, so a `\` + CRLF string continuation is joined like `\` + LF. A file the lexer cannot classify is `KNP1-PARSE`; an unreadable listed file is fatal `KNP1-READ`.
- **IDs and file names (fail closed).** Compared ASCII-case-insensitively as substrings of every string, byte-string and C-string literal, both as written and unescaped as rustc does (`\x2d`, `\u{…}`, line continuations; a `\u{…}` escape skips `_` without counting it, takes 1 to 6 hex digits and ends only at `}`). A literal with an escape rustc would reject (`\u` without braces or with no or more than six digits, a value outside `0..=0x10FFFF` or a surrogate, `\x` without two hex digits, an unknown escape) is `KNP1-PARSE` (never waivable) and is never matched as written), and of the concatenation of the literals of one macro invocation as `concat!` renders them (strings and characters unescaped, integers as their decimal value, floats as written without `_` or suffix, `true`/`false`, a `-` sign), reported at the first of them: `concat!("tracim-lsq-circle-00", 7, ".json")` and `concat!("…007", '.', "json")` match.
- **Values.** Literals are normalised exactly before any length cap: strip `_` and the type suffix, then the leading and trailing zeros of the mantissa and the leading zeros of the exponent, keeping digits and a decimal exponent (no float conversion). A numeric literal the lexer accepted that still cannot be normalised (an exponent beyond ±999 999 999, more than 1 024 significant digits after stripping, an integer past `u128` in a non-decimal base) is `KNP1-PARSE` (never waivable), never nothing. Values are and compared on magnitude, so `-0.123456789012` matches. Significant digits are counted as written, from the first non-zero digit (trailing zeros count). Hex, octal and binary integers are compared through their decimal value.
- **Bits.** A float literal is parsed correctly rounded (and also as `f32` when so suffixed) and compared with the sign bit cleared. Any integer literal equal to a denylisted bit pattern is `KNP1-DATA-HEX` wherever it appears, not only as a `from_bits` argument, so a constant passed to `from_bits` indirectly is caught too.
- **One finding per literal.** A literal that matches both `KNP1-DATA-VALUE` and `KNP1-DATA-HEX` yields `KNP1-DATA-VALUE` only, so the canary yields exactly one finding (AC-1).
- **Excluded values** never produce a finding: neither their exact decimal value nor their bits.
- **Config:** `[data] denylist = "<path>"` (required, relative to `knp1-lint.toml`). `run` returns the findings and the verified denylist digest for the report.
- **Canary (deviation).** `UNC.RDL.LINT` is not built yet, so `fixtures/rdl/lint/` does not exist. `fixtures/knp1/data/make_fixtures.py` stands in for it with the same schema: a denylist from hand-chosen values, IDs and file names, a canary workspace with one planted constant, and the canary with it removed. This stand-in is replaced when `UNC.RDL.LINT` is built: AC-1 then runs on `fixtures/rdl/lint/canary_crate/` with `fixtures/rdl/lint/expected_denylist.json`, and the stand-in denylist and canary are removed (maintainer decision).
- **Performance.** The 300 000-line budget is not measured [unverified].
- **What a token walk cannot reach** (accepted gaps, owned elsewhere):
  - `concat!` arguments that are themselves macro calls (`concat!(m!(), …)`), `stringify!`, `include_str!` contents and other macro-produced text: macro expansion is `KRN.NUM.LINT.TYPED`'s (expanded workspace code); `KRN.NUM.LINT.SYN` already reports `include*!` of non-source files.
  - A value split across expressions (`0.123456 + 0.000000789012`), built by a `const fn`, parsed from a string at run time, or assembled from bits (`f64::from_bits(a | b)`): only literal tokens are compared. Evaluating constants needs the compiler (`KRN.NUM.LINT.TYPED`); the reference-result comparison itself is the CLI's validation step (`KRN.NUM.LINT.CLI`).
  - A literal with fewer than 12 significant digits whose bits do not equal a denylisted pattern (by the spec's threshold).

## Test fixtures

The canary crate and expected denylist under `fixtures/rdl/lint/` (published by `UNC.RDL.LINT`); `fixtures/knp1/data/` for literal forms, short literals, file names and a corrupt denylist; project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add module `data`. Load `rdl-denylist.json` (produced by UNC.RDL.LINT: `version`, `ids`, `files`, `values[{decimal: [..], hex}]`, `excluded`, `digest`); verify schema and SHA-256 digest over RFC 8785 JSON, else fail closed `KNP1-DATA-DENYLIST`. Over every closure file (token walk of literals, including macro token streams): report `KNP1-DATA-ID`/`KNP1-DATA-FILE` for string literals containing a denylisted ID or file name; `KNP1-DATA-VALUE` for numeric literals with >= 12 significant digits whose exact normalised decimal equals a denylisted decimal; `KNP1-DATA-HEX` for float literals or `f64::from_bits` arguments whose bits equal a denylisted hex literal. Never flag `excluded` values. Normalise literals exactly (strip `_` and suffixes) without float conversion for the decimal match. Tests: the UNC.RDL.LINT canary (positive and negative) and `fixtures/knp1/data/` for AC-1..AC-5 of spec node KRN.NUM.LINT.DATA. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`, bounded parsing.

## Open questions

- `UNC.RDL.LINT` asked KRN owners to confirm they consume `rdl-denylist.json`: confirmed by this leaf (`uses_artifacts: [UNC.RDL.LINT]`).
- The pi digit strings in `KRN.NUM.FMT.ANG` are longer than any denylist rendering and are matched only as whole literals, so they cannot match a 17-digit entry; confirm no RDL reference value is written as a long pi string.

## Children

_Leaf: none._
