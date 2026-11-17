---
id: IO.QIF.JSON.DIGEST
title: Canonical QIF-JSON bytes and document digest
level: 4
parent: IO.QIF.JSON
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.JSON-001]
covers: []
depends_on: [IO.QIF.JSON.PROJ, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.JSON.DIGEST: Canonical QIF-JSON bytes and document digest

## Purpose

Serialises a projected QIF-JSON document as RFC 8785 canonical bytes through `fid-canon` and computes its SHA-256, so the same QIF document state gives the same digest on every platform, whether it came from XML or JSON. TRUST and the QIF commands use this digest; it is a library function, not a registry command.

## Scope

**In:**
- `qif_canonical_bytes(doc) -> Vec<u8>` and `qif_digest(doc) -> Digest` (`sha256:<hex>`)
- Cross-platform conformance vectors (document, canonical bytes, digest)

**Out:**
- The projection (`IO.QIF.JSON.PROJ`) and canonical JSON rules in general (`CORE.JRN.CANON`)
- Signing (TRUST)

## Requirements

- **REQ-IO.QIF.JSON.DIGEST-001** The canonical bytes shall be produced by `fid-canon` from the lossless projection, with no number other than 32-bit integers emitted as a JSON number.
- **REQ-IO.QIF.JSON.DIGEST-002** For each conformance vector, the canonical bytes and digest shall be identical on Windows, Linux and macOS (D0).
- **REQ-IO.QIF.JSON.DIGEST-003** A document imported from QIF-XML and the same document imported from its lossless QIF-JSON export shall have the same digest.
- **REQ-IO.QIF.JSON.DIGEST-004** The digest shall be formatted as `sha256:` followed by 64 lowercase hex digits.

## Acceptance criteria

- **AC-1** Given the vectors in `fixtures/io/qif/json/digest/vectors.json` (at least 20 documents), when run on the three CI platforms, then every canonical byte string and digest matches the vector.
- **AC-2** Given `fixtures/io/qif/json/digest/doc.qif`, when imported from XML, exported as QIF-JSON, re-imported and digested, then both digests are equal.
- **AC-3** Given two documents differing only in a decimal written `1.5` versus `1.50`, then their digests differ.

## Design

- **Registry commands:** none (library; R1 by nature).
- **Data structures:** `Digest` from `fid-canon`.
- **Algorithm:** `proj::to_json` → `fid-canon` canonicalise → SHA-256.
- **Errors:** propagated `fid-canon` errors (`input_limit`, `duplicate_key`).
- **Determinism:** D0.
- **Performance budget:** 100 MB document digested in < 2 s [unverified until measured].
- **Crate:** `fid-qif` (module `json::digest`).

## Test fixtures

`fixtures/io/qif/json/digest/` vectors generated once and checked by an independent RFC 8785 implementation in another language (CC-BY-4.0).

## Build prompt

> In crate `fid-qif`, add module `json::digest`: `qif_canonical_bytes(&Model)` = `json::proj::to_json` serialised by `fid-canon` (RFC 8785), and `qif_digest` = SHA-256 formatted `sha256:<64 lowercase hex>`. Lexical numbers stay strings; only 32-bit integers are JSON numbers. Commit at least 20 conformance vectors checked against an independent RFC 8785 implementation and run them on Windows, Linux and macOS. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-3.

## Open questions

- CORE snapshots may not call this IO function (CORE is a lower layer, CONSTITUTION §2.1); CORE hashes snapshot bytes with `CORE.JRN.CANON` directly, and the two agree because this leaf uses the same crate.

## Children

_Leaf: none._
