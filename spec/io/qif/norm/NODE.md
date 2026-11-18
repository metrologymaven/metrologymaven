---
id: IO.QIF.NORM
title: The qif-norm normaliser and round-trip equality check
level: 3
parent: IO.QIF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-qif]
satisfies: [REQ-IO.QIF-006, REQ-IO.QIF-007]
covers: []
depends_on: [IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.QIF.NORM: The qif-norm normaliser and round-trip equality check

## Purpose

Defines and implements `qif-norm@1`, the published rule list under which two QIF-XML documents count as equal, and the comparison function that the QIF round-trip gate and `IO.CORP` use. Exact canonical-XML equality is not claimed; this normaliser states exactly what may differ.

## Scope

**In:**
- The versioned rule list (data file plus prose in the generated docs)
- `normalise(xml_bytes) -> canonical bytes` and `equal_under(norm, a, b) -> Result<Diff>`
- A first-difference report (XPath-like location, both values)
- The QIF-XML → QIF-JSON → QIF-XML round-trip test harness over a corpus manifest

**Out:**
- XML parsing hardening (reuses the reader from `IO.QIF.XML`; until it exists, a minimal hardened parser behind the same flags)
- Corpus contents (`IO.CORP`)

## Requirements

- **REQ-IO.QIF.NORM-001** `qif-norm@1` shall consist of exactly these rules: Canonical XML 1.1 form; XSD whitespace collapse for list and atomic simple-type content; removal of whitespace-only text between elements; namespace prefixes rewritten to a fixed prefix table.
- **REQ-IO.QIF.NORM-002** The rule list shall be stored as a versioned data file, and a CI check shall fail if the file changes without its version increasing.
- **REQ-IO.QIF.NORM-003** When two documents are compared, the function shall return `Equal` or the first difference with its element path, line in each input and both values.
- **REQ-IO.QIF.NORM-004** The normaliser shall not change lexical forms of numeric values (`1.50` and `1.5` are different).
- **REQ-IO.QIF.NORM-005** For every file in the QIF P0-subset corpus manifest, the round-trip harness shall report XML → JSON → XML equality under `qif-norm@1`, and the CI gate shall fail on any inequality.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/norm/ws_a.qif` and `ws_b.qif` (same content, different indentation and attribute order), when compared under `qif-norm@1`, then the result is `Equal`.
- **AC-2** Given `fixtures/io/qif/norm/dec_a.qif` (`<Value>1.50</Value>`) and `dec_b.qif` (`<Value>1.5</Value>`), then the result is a difference at the path of `Value` with both lexical values.
- **AC-3** Given `fixtures/io/qif/norm/list_ws.qif` where a `ListDoubleType` value has runs of spaces and newlines versus single spaces, then `Equal`.
- **AC-4** Given `fixtures/io/qif/norm/prefix_a.qif` (prefix `q:`) and `prefix_b.qif` (default namespace), then `Equal`.
- **AC-5** (error path) Given a rule-file edit without a version bump, when the CI check runs, then it fails with `norm_version_not_bumped`.
- **AC-6** Given the P0-subset corpus manifest (`IO.CORP`) and a stub projection, when the harness runs, then it emits a JSON report with one row per file and the gate's exit code reflects any inequality.

## Design

- **Registry commands:** none (library and CI harness; equality results are R1).
- **Data structures:** `NormRules { version: "qif-norm@1", rules: [...] }` loaded from `qif-norm.toml`; `Diff { path, line_a, line_b, a, b }`.
- **Algorithm:** parse with DTD and entities disabled; C14N 1.1 serialisation (attribute sort, namespace rewriting from a fixed table keyed by namespace URI); for elements whose IR type is list or atomic simple, collapse whitespace per XSD `collapse`; drop whitespace-only text nodes between elements; compare streams event by event to locate the first difference. Uses the Schema IR from `IO.QIF.GEN` to know which elements are simple-typed; until available, a generated table checked in as data.
- **Errors:** `malformed{at}` from the parser; `norm_version_not_bumped` in CI.
- **Determinism:** D0.
- **Performance budget:** 100 MB QIF normalised in < 5 s [unverified until measured].
- **Crate:** `fid-qif` (module `norm`).

## Test fixtures

Hand-written minimal QIF 3.0 documents under `fixtures/io/qif/norm/` (CC-BY-4.0), plus the DMSC sample files referenced by the `IO.CORP` manifest (Boost licence [unverified]).

## Build prompt

> In crate `fid-qif`, module `norm`, implement the `qif-norm@1` normaliser and comparator. Rules (data file `qif-norm.toml`, versioned): Canonical XML 1.1; XSD whitespace collapse for list and atomic simple-type content (element types from a generated table); drop whitespace-only text between elements; rewrite namespace prefixes from a fixed table. Never change numeric lexical forms. API: `normalise(&[u8]) -> Result<Vec<u8>, IoError>`, `compare(a, b) -> Result<Equality, IoError>` returning the first difference (element path, lines, values). Add a CI check that fails `norm_version_not_bumped` if the rule file changes without its version changing, and a round-trip harness that reads a corpus manifest and reports per-file equality as JSON. Parser: no DTD, no external entities. CONSTITUTION §8: no panics, no unwrap; KNP-style determinism, no std HashMap. Tests AC-1..AC-6 with fixtures under `fixtures/io/qif/norm/`.

## Open questions

- Is namespace-prefix rewriting needed, or does C14N 1.1 already make prefix choice irrelevant for equality? Kept explicit to make the rule list self-describing.

## Children

_Leaf: none._
