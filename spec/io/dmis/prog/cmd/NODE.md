---
id: IO.DMIS.PROG.CMD
title: DMIS program import command, strict mode and import report
level: 4
parent: IO.DMIS.PROG
kind: feature
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.PROG-001, REQ-IO.DMIS.PROG-002, REQ-IO.DMIS.PROG-003]
covers: []
depends_on: [IO.DMIS.PROG.PLAN, IO.DMIS.DIAL, IO.REG.ENV]
uses_artifacts: [IO.CORP.MANIF]
side_effect: W2
determinism: D0
cli: [fid import dmis-program]
mcp: [import_dmis_program]
journal: [import.dmis_program@1]
updated: 2026-11-17
---

# IO.DMIS.PROG.CMD: DMIS program import command

## Purpose

`import.dmis_program@1` parses a DMIS input program (with an optional dialect), builds the program model, maps it to QIF features, characteristics, datums, resources and plan items, and commits everything in one transaction together with the stored program model. The import report lists every unsupported statement with its line and class; in strict mode one unsupported statement fails the import.

## Scope

**In:**
- `import.dmis_program@1` (W2) on the `IO.REG.ENV` skeleton: `source`, `dialect?` (id and version), `strict?`, `lenient_parse?`, `stage?`, `dry_run?`
- DMIS program format descriptor (extensions `.dmi .dms`, first statement `DMISMN` as signature [unverified])
- Storing the source as a blob and the `DmisProgram` model as a derived blob for `CMM.GEN.DMIS`
- Import report: counts per created type, unsupported statements (line, class or keyword, reason), unresolved labels, conditional plan items, dialect used
- Journal: dialect `{id, version, digest}` when used

**Out:**
- Parsing, model and mapping (siblings and `IO.DMIS.PARSE`)
- Conversion to a Command Program (`CMM.GEN.DMIS`)

## Requirements

- **REQ-IO.DMIS.PROG.CMD-001** When a DMIS program is imported, every unsupported statement shall be listed in the import report with its line number, statement class or keyword, and reason.
- **REQ-IO.DMIS.PROG.CMD-002** If `strict` is set and the program contains an unsupported statement, then the import shall fail with `dmis_unsupported` listing them and add nothing.
- **REQ-IO.DMIS.PROG.CMD-003** QPIds present in the program shall appear unchanged on the committed QIF entities.
- **REQ-IO.DMIS.PROG.CMD-004** A successful import shall commit all QIF entities and the program model in one transaction producing one journal event that records the dialect used, if any.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/parse/gexec/program.dmi`, when `fid import dmis-program --source <handle> --json` runs, then features, characteristics, resources and plan items are committed in one journal event and the report lists zero unsupported statements.
- **AC-2** Given `fixtures/io/dmis/prog/model/vendor_stmt.dmi`, then the import succeeds and lists the vendor statement with line and keyword; with `--strict`, it fails `dmis_unsupported` and nothing is journaled.
- **AC-3** Given `fixtures/io/dmis/dial/uses_vndx.dmi` with `--dialect test-vendor@1.0.0`, then the statement is modelled and the journal records the dialect ID, version and digest.
- **AC-4** Given `fixtures/io/dmis/prog/feat/qpids.dmi`, then committed entities carry the QPIds.
- **AC-5** Given every program entry of `corpus/dmis.toml`, then the import succeeds (non-strict) and unsupported counts match the manifest.
- **AC-6** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.dmis_program@1` (W2; params above; result `DmisImportReport`). CLI `fid import dmis-program`, MCP `import_dmis_program`.
- **Algorithm:** bytes → blob → `parse(bytes, dialect)` → model → FEAT → TOL → PLAN → strict check → stage → one transaction.
- **Errors:** `dmis_unsupported`, `dialect_not_found`, `malformed`, `limit_exceeded`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 10^4-statement program in < 2 s [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Fixtures of sibling leaves; `corpus/dmis.toml` program entries (`IO.CORP.MANIF`).

## Build prompt

> In crate `fid-io-dmis`, implement `import.dmis_program@1` (W2; CLI `fid import dmis-program`; MCP `import_dmis_program`) on the `fid-io-reg` skeleton: store the source blob, parse with the optional dialect (record `{id, version, digest}` in the journal), build the `DmisProgram` model, map features, tolerances/datums and resources/plan items to QIF, store the model as a derived blob, list every unsupported statement (line, class or keyword, reason), fail `dmis_unsupported` in `strict` mode, keep QPIds, and commit in one transaction or nothing. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-6.

## Open questions

- None beyond the parent's.

## Children

_Leaf: none._
