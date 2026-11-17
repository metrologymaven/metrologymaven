---
id: GDT.RES.MATRIX
title: Generated support matrix
level: 3
parent: GDT.RES
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-007]
covers: []
depends_on: [GDT.RES.SCHEMA]
side_effect: W1
determinism: D0
cli: [fid gdt export-support-matrix]
mcp: [gdt_export_support_matrix]
journal: [gdt.export_support_matrix@1]
updated: 2026-11-16
---

# GDT.RES.MATRIX: Generated support matrix

## Purpose

Generates the published support matrix (REQ-GDT-008) from a DMT table: one line per (dialect, edition set, characteristic, modifier combination) with its status and limitation codes. Because it is generated and never hand-edited, the project's public claim and the kernel's behaviour cannot drift apart. OUTR and the docs portal consume its output.

## Scope

**In:** matrix model built from a `DmtTable`; Markdown and JSON renderers; deterministic ordering; `gdt.export_support_matrix@1` writing to an approved local export root (W1, `fs_effect: write_root`, CONSTITUTION §3.1); a header naming the DMT version and digest; a CI check that the committed matrix equals a fresh generation.
**Out:** DMT schema and content (owned by `GDT.RES.SCHEMA`, `GDT.RES.ROWASME`, `GDT.RES.ROWISO`); publishing to the website (owned by `OUTR.DOCS`); parity reports across domains (owned by `ECO.PARITY`).

## Requirements

- **REQ-GDT.RES.MATRIX-001** The generator shall emit one entry per DMT row with dialect, edition set, characteristic, modifiers, status and limitation codes, sorted by row key.
- **REQ-GDT.RES.MATRIX-002** Each output shall begin with the DMT version and table digest it was generated from.
- **REQ-GDT.RES.MATRIX-003** Generation shall be deterministic: the same table shall yield byte-identical Markdown and JSON.
- **REQ-GDT.RES.MATRIX-004** If the committed matrix differs from a fresh generation of the committed DMT, then the CI check shall fail and name the first differing line.
- **REQ-GDT.RES.MATRIX-005** The export command shall write only into an approved local export root given as a handle, and shall be classed X1 if the root is not local.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dmt/synthetic_table_v1.json`, when `fid gdt export-support-matrix --dmt <ref> --format md --out <root-handle> --json` runs, then the written file equals `fixtures/gdt/matrix/expected_v1.md` byte for byte and the result names its path within the root.
- **AC-2** Same with `--format json` equals `expected_v1.json`.
- **AC-3** Given the table with rows shuffled in the file, then both outputs are unchanged.
- **AC-4** (error path) Given a raw path instead of a root handle, then the command fails with `not_an_export_root` and writes nothing.
- **AC-5** (CI) Given a committed matrix with one status edited by hand, when the CI check runs, then it fails naming that line.
- **AC-6** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL` and the file digest matches.

## Design

- **Registry command(s):** `gdt.export_support_matrix@1`, W1 with `fs_effect: write_root`; params `dmt: TableRef`, `format: md | json`, `out: ExportRootHandle`; result `{ path_in_root, digest }`; `--dry-run` returns content without writing.
- **Data structures:** `MatrixEntry { key, dialect, editions, characteristic, modifiers, status, limitations }`.
- **Algorithm:** iterate rows in key order, render; Markdown uses a fixed column set and escapes `|`.
- **Errors:** `not_an_export_root`, `dmt_not_loaded`, `write_failed`.
- **Determinism:** D0.
- **Performance budget:** 5 000 rows in < 100 ms [unverified].
- **Crate:** `fid-gdt` (not the kernel: it writes files).

## Test fixtures

Reuses the synthetic table; expected outputs are generated once, reviewed by a human, and committed (CC0).

## Build prompt

> In crate `fid-gdt`, implement the support-matrix generator and registry command `gdt.export_support_matrix@1` (side-effect W1, `fs_effect: write_root`; CLI `fid gdt export-support-matrix`; MCP `gdt_export_support_matrix`; `--json`, `--dry-run`). Input: a loaded `DmtTable` from `fid-gps::dmt`. Output: Markdown or JSON, one entry per row (dialect, edition set, characteristic, modifiers, status, limitation codes), sorted by key, headed by DMT version and digest, byte-identical for identical input. Writes only through an approved export-root handle (`not_an_export_root` otherwise; never a raw path). Add a CI test that regenerates the matrix from the committed DMT and fails naming the first differing line. Tests per the acceptance criteria, including exact replay = IDENTICAL. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never hand-edit generated output.

## Open questions

- None.

## Children

_Leaf: none._
