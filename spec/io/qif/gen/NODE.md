---
id: IO.QIF.GEN
title: XSD to Schema IR code generator for QIF 3.0 types and JSON Schema
level: 3
parent: IO.QIF
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.QIF-001, REQ-IO.QIF-002, REQ-IO.QIF-012]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.GEN: XSD to Schema IR code generator

## Purpose

A build-time tool that parses the pinned DMSC QIF 3.0 XSD set into a Schema IR and emits the Rust model crate and the QIF-JSON JSON Schema 2020-12 from that one IR. The schema is about 100k lines across about 20 modules, so hand-written bindings are not viable [unverified: size from concept doc 04].

## Scope

**In:**
- Pinning the XSD set by SHA-256 (manifest of every file)
- XSD parsing into a Schema IR: complex and simple types, sequence, choice, all, substitution groups, abstract heads, facets, `key`/`keyref`, annotations, `n`/`count` derived attributes
- IR normalisation: flattened extension and restriction chains, deterministic names for anonymous types
- Emitters: Rust types (serde-free, own visitor traits), JSON Schema 2020-12 for QIF-JSON, a type reference document reusing DMSC's Boost-licensed annotations
- A CI gate that regenerates and diffs against the committed output

**Out:**
- XML parsing and validation of instance documents (`IO.QIF.XML`)
- The JSON projection rules themselves (`IO.QIF.JSON`; the generator implements its naming and choice rules as data handed over by that node)
- ISO 23952 prose (never used)

## Requirements

- **REQ-IO.QIF.GEN-001** The generator shall refuse to run if any XSD file's SHA-256 differs from the pinned manifest, failing with error `xsd_digest_mismatch`.
- **REQ-IO.QIF.GEN-002** When run twice on the same pinned XSDs and generator version, the generator shall produce byte-identical Rust sources, JSON Schema and docs.
- **REQ-IO.QIF.GEN-003** CI shall fail if committed generated sources differ from a fresh generation, so generated files are never edited by hand.
- **REQ-IO.QIF.GEN-004** The Schema IR shall represent every XSD construct used in the pinned set, and the generator shall fail with error `xsd_construct_unsupported` naming file and line for any construct it cannot represent.
- **REQ-IO.QIF.GEN-005** The generator shall emit one JSON Schema 2020-12 document for QIF-JSON whose definitions correspond one-to-one with IR types.
- **REQ-IO.QIF.GEN-006** Every generated type shall be emitted (full model), and the P0 subset (MBD characteristics and Results) shall be the subset whose round-trip is CI-gated in P0.

## Interfaces

- **Offers:** the `fid-qif-model` crate (generated), `qif-json.schema.json`, the Schema IR as a library for `IO.QIF.XML` (validation tables) and `IO.QIF.JSON` (projection tables).
- **Consumes:** the pinned DMSC XSD set (`qif-community`, Boost licence [unverified: confirm licence file]).

## Decomposition hints

PIN (manifest and fetch), PARSE (XSD to IR), NORM (IR flattening and naming), RUST (emitter), JSCHEMA (emitter), DOCS (annotation emitter).

## Open questions

- Whether the normative XSLT checks (22 in QIF 3.0 per concept doc 04 [unverified]) are re-expressed from the IR or run as XSLT in a separate process: settled in `IO.QIF.XML.XSLT` (re-expressed in Rust, no C dependency). The XSLT files are pinned with role `xslt` by `IO.QIF.GEN.PIN`.
- The JSON projection rules are read as a published data file (`uses_artifacts: [IO.QIF.JSON.PROJ]` on `IO.QIF.GEN.JSCHEMA`), not as a code dependency, which avoids a GEN/JSON cycle.
- The `ExtensionBag` container type lives in the small `fid-qif-ir` crate so generated types can carry it; its rules belong to `IO.QIF.IDS.EXT`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.QIF.GEN.PIN | Pinned XSD manifest and digest check (leaf) | 001 | none |
| IO.QIF.GEN.PARSE | XSD parser into the Schema IR (leaf) | 004 | none |
| IO.QIF.GEN.FLAT | Schema IR normalisation, naming and validation tables (leaf) | 004 005 | none |
| IO.QIF.GEN.RUST | Rust model emitter for the fid-qif-model crate (leaf) | 006 | none |
| IO.QIF.GEN.JSCHEMA | QIF-JSON JSON Schema and type reference emitter (leaf) | 005 | none |
| IO.QIF.GEN.GATE | Generator determinism and regeneration CI gate (leaf) | 002 003 | none |
