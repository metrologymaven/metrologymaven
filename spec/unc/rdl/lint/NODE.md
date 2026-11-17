---
id: UNC.RDL.LINT
title: Kernel reference-data denylist
level: 3
parent: UNC.RDL
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-009]
covers: []
depends_on: [UNC.RDL.FORMAT, UNC.RDL.FREEZE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.RDL.LINT: Kernel reference-data denylist

## Purpose

Produces the denylist that the KNP-1 lint (owned by `KRN.NUM`, REQ-KRN.NUM-009) uses to fail the build when kernel source references RDL data: dataset IDs, input file names, and reference numeric constants in decimal and hexadecimal forms. It also provides a canary test that proves the lint actually fails on a planted reference. The lint engine is KRN's; the knowledge of what to ban is RDL's.

## Scope

**In:**
- Denylist generator over all tier X, G and O records plus tier B IDs: IDs, blob file names, and reference values rendered as decimal strings (8 to 17 significant digits) and IEEE-754 hex literals
- Output file `rdl-denylist.json` with version and digest, regenerated on every RDL change
- Canary crate fixture with one planted reference constant, used in CI to assert the lint fails

**Out:**
- The lint engine and its CI wiring (owned by `KRN.NUM`)

## Requirements

- **REQ-UNC.RDL.LINT-001** The generator shall emit every dataset ID, every input blob file name, and every reference output value as decimal strings with 8 through 17 significant digits and as a hexadecimal f64 literal.
- **REQ-UNC.RDL.LINT-002** Reference values whose magnitude is a common constant (0, ±1, ±0.5, ±2, powers of ten from 1e-12 to 1e12) shall be excluded from the denylist and listed in its `excluded` section.
- **REQ-UNC.RDL.LINT-003** The denylist shall be sorted, versioned and digested so that the same RDL state yields the same file on every platform.
- **REQ-UNC.RDL.LINT-004** CI shall run the KNP-1 lint against the canary fixture with the current denylist, and the job shall fail if the lint does not report the planted constant.

## Acceptance criteria

- **AC-1** Given the records in `fixtures/rdl/lint/records/` (3 datasets), when the denylist is generated, then it equals `fixtures/rdl/lint/expected_denylist.json` byte for byte.
- **AC-2** Given a record with reference value 1.0 and another with 0.123456789012, when generated, then 1.0 is in `excluded` and 0.123456789012 appears as decimal strings of 8..17 digits and as its hex literal.
- **AC-3** (canary) Given `fixtures/rdl/lint/canary_crate/` containing `const R: f64 = <a planted reference value>;`, when the KNP-1 lint runs with the generated denylist, then the lint reports that constant and exits non-zero.
- **AC-4** (canary negative) Given the canary crate with the constant removed, then the lint passes.
- **AC-5** (portability) Generating on wasm32, x86-64 and ARM64 hosts yields identical bytes.

## Design

- **Registry command(s):** none; a build-time tool `rdl-denylist` in crate `fid-rdl` (binary target), algorithm `rdl.lint.denylist@1.0.0`.
- **Data structures:** `Denylist { version, ids: Vec<String>, files: Vec<String>, values: Vec<{decimal: Vec<String>, hex: String}>, excluded: Vec<String>, digest }`.
- **Algorithm:** load records via `UNC.RDL.FORMAT` (tier B: IDs only), render each reference value with shortest round-trip decimal and with fixed 8..17 significant digits (correctly rounded), dedupe, sort, canonicalize, digest.
- **Errors:** `record_invalid{id}`.
- **Determinism:** D0.
- **Performance budget:** 100,000 reference values in under 2 s [unverified until measured].
- **Crate:** `fid-rdl`.

## Test fixtures

`fixtures/rdl/lint/`: three synthetic records, the expected denylist, and a canary crate (CC-BY-4.0 data; canary code under the project licence).

## Build prompt

> In crate `fid-rdl`, add a binary target `rdl-denylist` (algorithm `rdl.lint.denylist@1.0.0`) that loads all RDL records (tier B contributes IDs only) and writes `rdl-denylist.json`: dataset IDs, input blob file names, and every reference output value rendered as correctly rounded decimal strings with 8 through 17 significant digits plus its IEEE-754 hex literal. Exclude common constants (0, ±1, ±0.5, ±2, powers of ten 1e-12..1e12) into an `excluded` list. Dedupe, sort, add version and SHA-256 digest over RFC 8785 JSON.
> Add a CI job that runs the `KRN.NUM` KNP-1 lint over `fixtures/rdl/lint/canary_crate/` (which plants one reference constant) and fails if the lint does NOT report it; and passes when the constant is removed. The lint engine itself belongs to `KRN.NUM`; do not reimplement it.
> Tests: expected denylist bytes for `fixtures/rdl/lint/records/`; exclusion and rendering of the two sample values; canary positive and negative; identical output on wasm32, x86-64, ARM64 hosts. No panics, no `unwrap`.

## Open questions

- REQ-UNC.RDL-009 and REQ-KRN.NUM-009 state the same rule from two sides; this leaf takes the data side only. KRN owners should confirm they consume `rdl-denylist.json`.

## Children

_Leaf: none._
