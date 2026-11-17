---
id: QUAL.VAP.GEN
title: Validation pack generator command
level: 3
parent: QUAL.VAP
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.VAP-003, REQ-QUAL.VAP-005]
covers: []
depends_on: [QUAL.VAP.TPL, CORE.BUS.POL.FS, CORE.REG.DEF]
uses_artifacts: [QUAL.VAP.PROTO, QUAL.VAP.CHECK, QUAL.VAP.SUPQ, QUAL.EVID.LAYOUT]
side_effect: W1
determinism: D0
cli: [fid validate generate-pack]
mcp: [validate_generate_pack]
journal: [validate.generate_pack@1]
updated: 2026-11-17
---

# QUAL.VAP.GEN: Validation pack generator command

## Purpose

`validate.generate_pack@1` renders the installed release's VAP template set for a customer's intended-use selection into an approved export root, stamping every document with the release version, the evidence-statement digest, the selection and the statement that validation and compliance decisions belong to the customer. It runs offline from templates shipped with the release.

## Scope

**In:**
- Command with parameters: intended-use IDs, export-root handle, folder name, format (`md`, `odt` or both)
- Stamp block on every document: release version, evidence-statement digest of the installed release, selection, generation journal entry reference, customer-responsibility statement
- Pack index `pack.json` listing every document with its SHA-256
- Atomic write; refuse to overwrite

**Out** (and which node owns it instead):
- Template content (owned by `QUAL.VAP.TPL`, `QUAL.VAP.PROTO`, `QUAL.VAP.CHECK`, `QUAL.VAP.SUPQ`)
- Periodic-review report (owned by `QUAL.VAP.REVIEW`)

## Requirements

- **REQ-QUAL.VAP.GEN-001** When `validate.generate_pack@1` runs with intended-use IDs and an export-root handle, the system shall write every template of the installed set, rendered for that selection, into a new folder in that root.
- **REQ-QUAL.VAP.GEN-002** Every generated document shall carry the release version, the installed evidence-statement digest, the selection and the statement that validation and compliance decisions belong to the customer.
- **REQ-QUAL.VAP.GEN-003** If an intended-use ID is not in the catalogue, then the command shall fail with `validate.unknown_intended_use` and write nothing.
- **REQ-QUAL.VAP.GEN-004** If the target folder exists, then the command shall fail with `validate.export_exists` and write nothing.
- **REQ-QUAL.VAP.GEN-005** The command shall write `pack.json` listing every document and its SHA-256, and journal that file's digest as the result digest.

## Acceptance criteria

- **AC-1** Given the fixture template set and evidence statement `fixtures/qual/vap/gen/install/`, when `fid validate generate-pack --uses transcription_reports --root out --name vap-1.4 --format both --json` runs, then the folder holds 18 files (9 templates in two formats) plus `pack.json`, and `pack.json` equals the golden file.
- **AC-2** Given AC-1's output, then every document contains the stamp block with version `1.4.0`, the evidence digest and the responsibility statement.
- **AC-3** (error path) Given `--uses laser_tracking`, then it fails with `validate.unknown_intended_use` and writes nothing.
- **AC-4** (error path) Given an existing folder, then it fails with `validate.export_exists`.
- **AC-5** (MCP) Given AC-1 inputs, when `validate_generate_pack` is called, then the result equals the CLI JSON.

## Design

- **Registry command(s):** `validate.generate_pack@1` (W1, `fs_effect: write_root`; params `{ uses: Vec<string>, root: handle, name: string, format: md|odt|both }`; result `{ folder_handle, pack_digest, documents }`).
- **Data structures:** `PackIndex { release, evidence_statement_digest, uses, documents: Vec<{name, sha256}> }`.
- **Algorithm:** load installed templates and catalogue; render each with `vap::tpl`; insert stamp block; write to a temporary folder; rename.
- **Errors:** `validate.unknown_intended_use`, `validate.export_exists`, `validate.root_denied`, `validate.templates_missing`.
- **Determinism:** D0.
- **Performance budget:** under 3 s.
- **Crate:** `fid-qual` (module `vap::gen`).

## Test fixtures

`fixtures/qual/vap/gen/install/` with a copy of the template set and a synthetic evidence statement; CC-BY-4.0.

## Build prompt

> In crate `fid-qual`, add module `vap::gen` and register `validate.generate_pack@1` (W1, fs_effect write_root; CLI `fid validate generate-pack --uses <ids> --root <handle> --name <n> [--format md|odt|both]`, MCP `validate_generate_pack`). Load the installed template set and intended-use catalogue; refuse unknown IDs; render every template with `vap::tpl` for the selection; add to each document a stamp block (release version, installed evidence-statement digest, selection, and the statement that validation and compliance decisions belong to the customer); write atomically into a new folder in the approved export root (refuse if it exists) with `pack.json` (documents and SHA-256); journal the `pack.json` digest. No panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.VAP.GEN.

## Open questions

- None.

## Children

_Leaf: none._
