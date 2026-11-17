---
id: CORE.JRN.CANON
title: Canonical JSON encoding and digests
level: 3
parent: CORE.JRN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN-002, REQ-CORE.JRN-003, REQ-CORE.JRN-004]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.JRN.CANON: Canonical JSON encoding and digests

## Purpose

One small, dependency-light crate that turns a typed JSON value into RFC 8785 (JCS) canonical bytes and a SHA-256 digest, with the project's number rules: 64-bit integers as decimal strings, bit-significant f64 as the hex of their IEEE 754 bits, and no NaN or infinity anywhere. The journal, the registry manifest, the verifier and replay all hash through this crate, so they agree byte for byte.

## Scope

**In:** the typed value model for hashed structures, RFC 8785 serialisation, integer and f64 encoding rules, non-finite rejection, duplicate-key rejection, digest formatting, input limits, cross-language conformance vectors.
**Out:** the entry schema and salted commitments (`CORE.JRN.ENTRY`); the chain (`CORE.JRN.CHAIN`); the TypeScript or other verifier implementations (`TRUST`), which use this leaf's vectors.

## Requirements

- **REQ-CORE.JRN.CANON-001** The encoder shall serialise any value of its typed JSON model to the canonical form defined by RFC 8785 (sorted object keys by UTF-16 code units, no insignificant whitespace, ECMAScript number formatting).
- **REQ-CORE.JRN.CANON-002** The typed model shall encode every 64-bit signed or unsigned integer as a JSON string of decimal digits with no leading zeros and a leading minus sign only for negative values.
- **REQ-CORE.JRN.CANON-003** The typed model shall encode a bit-significant f64 as a JSON string `0x` followed by the 16 lowercase hex digits of its IEEE 754 bits, so that -0.0 and 0.0 differ.
- **REQ-CORE.JRN.CANON-004** If any number in a value is NaN or infinite, then the encoder shall return a typed `non_finite_number` error and produce no bytes.
- **REQ-CORE.JRN.CANON-005** If a parsed input object contains a duplicate key, then the parser shall return a typed `duplicate_key` error.
- **REQ-CORE.JRN.CANON-006** The crate shall compute a digest as SHA-256 over the canonical bytes and format it as `sha256:` followed by 64 lowercase hex digits.
- **REQ-CORE.JRN.CANON-007** If an input exceeds 64 MiB or a nesting depth of 128, then the parser shall fail closed with a typed `input_limit` error.
- **REQ-CORE.JRN.CANON-008** The crate shall ship a conformance vector set, and an independent second implementation in another language shall reproduce every vector's bytes and digest in CI.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/canon/vectors.json` (at least 200 input/expected-bytes/expected-digest triples, including Unicode key ordering, escapes, and boundary integers 0, 2^53, 2^64-1, -2^63), when each input is encoded, then bytes and digest match exactly.
- **AC-2** Given the same vectors, when run through the TypeScript conformance runner in CI, then every result matches the Rust result.
- **AC-3** Given f64 values -0.0 and 0.0, when encoded as bit-significant, then the outputs are `"0x8000000000000000"` and `"0x0000000000000000"`.
- **AC-4** (error path) Given a value containing NaN, when encoded, then the result is `non_finite_number` and no bytes are produced.
- **AC-5** (error path) Given `{"a":1,"a":2}`, when parsed, then the result is `duplicate_key`.
- **AC-6** Given a 10,000-deep nested array, when parsed, then the result is `input_limit` and the process does not overflow its stack.

## Design

- **Registry command(s):** none; library crate.
- **Data structures:** `enum CValue { Null, Bool(bool), Str(String), I64(i64), U64(u64), F64Bits(f64), Num(f64), Arr(Vec<CValue>), Obj(BTreeMap<String, CValue>) }` where `Num` is for non-bit-significant finite numbers printed per ECMAScript rules; `Digest` newtype.
- **Algorithm:** RFC 8785 sections 3.2.2 (number serialisation) and 3.2.3 (property sorting by UTF-16 code units), implemented from the RFC by reference to clause numbers only; iterative (non-recursive) parser and writer with explicit depth counter.
- **Errors:** `non_finite_number`, `duplicate_key`, `input_limit`, `invalid_utf8`, `syntax`.
- **Determinism:** D0; integer and string operations only, and ECMAScript-compatible shortest round-trip number printing.
- **Performance budget:** canonicalise 1 MB of entry JSON in < 20 ms [unverified until measured].
- **Crate:** `fid-canon`.

## Test fixtures

`fixtures/core/jrn/canon/vectors.json`, written for this project and cross-checked by two implementations; CC-BY-4.0. A TypeScript runner (`fixtures/core/jrn/canon/runner.ts`) is the second implementation.

## Build prompt

> Create the Rust crate `fid-canon` implementing RFC 8785 JSON canonicalisation for a typed value model `CValue` (Null, Bool, Str, I64, U64, F64Bits, Num, Arr, Obj with `BTreeMap` keyed by String but sorted for output by UTF-16 code units). Rules: I64/U64 serialise as decimal strings; F64Bits serialise as `"0x"` plus 16 lowercase hex digits of the IEEE bits; Num uses ECMAScript shortest round-trip formatting; NaN or infinity anywhere gives `non_finite_number`. Provide `to_canonical_bytes`, `digest -> "sha256:<hex>"`, and a bounded iterative parser (64 MiB, depth 128) that rejects duplicate keys. Ship `fixtures/core/jrn/canon/vectors.json` and a TypeScript runner reproducing them in CI. Tests: AC-1..AC-6. Constraints: no `unsafe`, deny unwrap/expect/panic in non-test code, no recursion on untrusted depth, dependencies limited to an allowlisted SHA-256 crate.

## Open questions

- `TRUST.CHAIN.CANON` is a deliberately independent verifier-side canonicaliser; it should consume this leaf's `vectors.json` as golden input, in addition to the TypeScript runner.
- Whether the ECMAScript number printing should be hand-written or taken from an allowlisted crate; either is acceptable if AC-1 and AC-2 pass.

## Children

_Leaf: none._
