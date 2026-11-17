---
id: TRUST.CHAIN.CANON
title: Independent RFC 8785 canonicaliser and golden vectors
level: 3
parent: TRUST.CHAIN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.CHAIN-007]
covers: []
depends_on: []
uses_artifacts: [CORE.JRN.CANON, CORE.JRN.ENTRY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.CHAIN.CANON: Independent RFC 8785 canonicaliser and golden vectors

## Purpose

A verifier-side implementation of JSON canonicalisation (RFC 8785, JCS) and of the journal's entry-hash rule, written without sharing code with CORE's journal writer. Two independent implementations checked against the same golden journals catch bugs that one implementation would silently agree with. It is the base of every chain check.

## Scope

**In:**
- Bounded JSON parser producing an ordered value tree (duplicate keys rejected)
- RFC 8785 serialisation: UTF-16 code-unit key ordering, ECMAScript number formatting, minimal string escaping
- The journal entry-hash function: SHA-256 over the canonical header (as published by CORE.JRN)
- Checks of the journal's numeric conventions (64-bit integers as decimal strings, bit-significant f64 as hex of IEEE 754 bits, no NaN or infinity)
- Golden vectors and a cross-check CI job against CORE's writer

**Out:**
- Chain walking and findings (`TRUST.CHAIN.LINKS`)
- The journal entry format itself (`CORE.JRN`)

## Requirements

- **REQ-TRUST.CHAIN.CANON-001** The canonicaliser shall produce output byte-identical to the RFC 8785 reference outputs for every vector in the golden set, including the RFC's published number-formatting examples [unverified: availability of a redistributable vector file].
- **REQ-TRUST.CHAIN.CANON-002** The parser shall reject duplicate object keys, lone surrogates, numbers outside the IEEE 754 double range, nesting deeper than 128 and inputs over 64 MiB with `canon.invalid`.
- **REQ-TRUST.CHAIN.CANON-003** The entry-hash function shall compute SHA-256 over the canonical form of the entry header exactly as CORE.JRN's published schema defines the header.
- **REQ-TRUST.CHAIN.CANON-004** The crate shall not depend on any CORE crate, and a CI layering check shall fail the build if it does.
- **REQ-TRUST.CHAIN.CANON-005** A CI cross-check shall hash every entry of the golden journals with both this crate and CORE's writer and fail on any difference.
- **REQ-TRUST.CHAIN.CANON-006** If a header holds a JSON number where the journal convention requires a decimal string or a hex-encoded f64, then the entry-hash function shall report `canon.convention` with the field path.

## Acceptance criteria

- **AC-1** Given `fixtures/canon/vectors/*.json` with expected `*.canon` outputs, when canonicalised, then every output matches byte for byte.
- **AC-2** Given golden journals `fixtures/journal/golden/{small,branchy,unicode}.jsonl` from CORE.JRN, when the cross-check job runs, then every entry hash from this crate equals CORE's.
- **AC-3** (error path) Given `fixtures/canon/bad/*` (duplicate key, lone surrogate, depth 129, `1e400`), then each fails with `canon.invalid` and none panics.
- **AC-4** (error path) Given a header with a raw 64-bit integer number, then `canon.convention` names its path.
- **AC-5** (fuzz) Given a differential fuzz target comparing this crate to CORE's canonicaliser for 10 minutes in CI, then no difference, crash or hang occurs.

## Design

- **Registry command(s):** none.
- **Data structures:** `Value` (ordered, with `Vec<(String, Value)>` objects); `EntryHeader` view.
- **Algorithm:** RFC 8785 §3 rules [unverified clause numbering]; number formatting via a shortest round-trip double-to-string algorithm (e.g. Ryu) adapted to ECMAScript rules; keys sorted by UTF-16 code units.
- **Errors:** `canon.invalid`, `canon.convention`.
- **Determinism:** D0.
- **Performance budget:** at least 100 MB/s canonicalisation on reference hardware [unverified until measured].
- **Crate:** `fid-chain` (module `canon`); no dependency on `fid-core*`.

## Test fixtures

- `fixtures/canon/vectors/`: RFC 8785 examples (if redistributable) plus generated vectors; `fixtures/canon/bad/`.
- `fixtures/journal/golden/*`: golden journals published by CORE.JRN (CC-BY-4.0).

## Build prompt

> Create crate `fid-chain` with module `canon`, an independent implementation of RFC 8785 JSON canonicalisation and of the journal entry-hash rule (SHA-256 over the canonical entry header per CORE.JRN's published schema). Do not depend on any `fid-core*` crate; add a layering check. Parser limits: 64 MiB input, depth 128; reject duplicate keys, lone surrogates and out-of-range numbers (`canon.invalid`). Check journal numeric conventions (64-bit integers as decimal strings, bit-significant f64 as hex IEEE 754 bits, no NaN/inf) and report `canon.convention` with the field path. Add a CI cross-check that hashes the golden journals with this crate and with CORE's writer, and a differential fuzz target. Tests: the five acceptance criteria. Constraints: CONSTITUTION §8 (no panics, bounded parsers, no unsafe), D0, builds for wasm32.

## Open questions

- If maintainers prefer one shared audited canonicaliser over independence (TRUST.CHAIN open question), this leaf becomes the cross-check only; that would need a decision.

## Children

_Leaf: none._
