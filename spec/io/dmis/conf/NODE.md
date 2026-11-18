---
id: IO.DMIS.CONF
title: Generated DMIS conformance table
level: 3
parent: IO.DMIS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-io-dmis]
built_pr: 107
satisfies: [REQ-IO.DMIS-008]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.DMIS.CONF: Generated DMIS conformance table

## Purpose

Generates the published DMIS conformance table from support declarations that the DMIS crates make in code, so the table can never claim more than the implementation does. It answers, per conformance module and level, whether import and export are supported, and lists the statements behind each answer.

## Scope

**In:**
- The support-declaration format: per statement keyword, `import: none|partial|full`, `export: none|partial|full`, conformance module and level tags, and a test ID proving the claim
- A generator producing `dmis-conformance.json` and a Markdown table
- A CI check that every `full` or `partial` claim names an existing test ID

**Out:**
- The parser and mappers (siblings), which only declare support
- Hosting the published table (ECO, OUTR)

## Requirements

- **REQ-IO.DMIS.CONF-001** The conformance table shall be generated only from the support declarations and shall never be edited by hand.
- **REQ-IO.DMIS.CONF-002** The table shall state, for each DMIS 5.3 conformance module and level, the import and export support as `none`, `partial` or `full`, where a module-level is `full` only if every statement tagged to it is `full`.
- **REQ-IO.DMIS.CONF-003** If a declaration claims `partial` or `full` without a test ID that exists in the test inventory, then the generator shall fail with `conformance_unproven` naming the statement.
- **REQ-IO.DMIS.CONF-004** Generating twice from the same declarations shall give byte-identical output.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/conf/decl_small.toml` (5 statements over 2 module-levels, one statement `partial`) and `fixtures/io/dmis/conf/tests.txt`, when the generator runs, then `dmis-conformance.json` shows module-level A `full` and B `partial`, and the Markdown table matches `fixtures/io/dmis/conf/expected.md` byte for byte.
- **AC-2** (error path) Given a declaration claiming `full` with test ID `t_missing`, then the generator fails `conformance_unproven{statement}`.
- **AC-3** Given the same inputs twice, then outputs are byte-identical.
- **AC-4** Given a CI diff check, when the committed table differs from a fresh generation, then CI fails.

## Design

- **Registry commands:** none (build-time generator; output is a published artefact, R0 to read).
- **Data structures:** `SupportDecl { keyword, import, export, modules: [ModuleLevel], tests: [TestId] }`; `ModuleLevel { module: String, level: u8 }`.
- **Algorithm:** load declarations (sorted by keyword), validate test IDs against an inventory produced by `cargo test -- --list`, aggregate per module-level with min over statements (`none` < `partial` < `full`), emit JSON (RFC 8785) and Markdown.
- **Errors:** `conformance_unproven`, `malformed` (declaration file).
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-io-dmis` (binary target `dmis-conformance`).

## Test fixtures

Small TOML declarations, a test-inventory text file and expected outputs under `fixtures/io/dmis/conf/` (CC-BY-4.0). Module names in fixtures are placeholders; the real module and level list is entered from the DMIS 5.3 standard by designator only [unverified: module list].

## Build prompt

> In crate `fid-io-dmis`, add a binary `dmis-conformance` that reads support declarations (TOML: keyword, import/export support `none|partial|full`, conformance module-level tags, test IDs), checks every non-`none` claim names a test present in an inventory file (else fail `conformance_unproven{statement}`), aggregates per module-level as the minimum over its statements, and writes `dmis-conformance.json` (RFC 8785 canonical) and a Markdown table, byte-identical across runs. Add a CI step that regenerates and diffs. No standards text in outputs, only designators and module names. No panics, no unwrap (CONSTITUTION §8); BTreeMap only. Tests AC-1..AC-4 with fixtures under `fixtures/io/dmis/conf/`.

## Open questions

- Whether a DMIS 5.3 conformance test suite exists beyond the 5.2-era NIST suite [unverified; carried from IO.DMIS].

## Children

_Leaf: none._
