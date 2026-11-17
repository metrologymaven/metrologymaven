---
id: UNC.RDL.GEN.CMD
title: The rdl.generate_dataset@1 command and family registry
level: 4
parent: UNC.RDL.GEN
kind: feature
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-001]
covers: []
depends_on: [UNC.RDL.GEN.HPREC, UNC.RDL.GEN.LSNULL, UNC.RDL.GEN.GEOM, UNC.RDL.GEN.STRESS, UNC.RDL.GEN.XCHECK, CORE.BUS.PIPE, CORE.REG.DEF]
side_effect: R1
determinism: D0
cli: [fid rdl generate-dataset]
mcp: [rdl_generate_dataset]
journal: [rdl.generate_dataset@1]
updated: 2026-11-17
---

# UNC.RDL.GEN.CMD: The rdl.generate_dataset@1 command and family registry

## Purpose

The registry command that runs one generator family for a seed and case count and returns the records. It stores nothing; freezing is a separate W3 step. An optional oracle-result file handle lets it return cross-checked records in one call.

## Scope

**In:**
- Family registry (sorted family IDs with versions), including `UNC.RDL.GEN.MZEQ` families once they exist
- `rdl.generate_dataset@1` (R1): params `family`, `family_version`, `seed`, `cases`, optional `oracle` import handle, `require_all`
- Listing families in the result of `--family list`

**Out:**
- Families (owned by the sibling leaves); storing and freezing (owned by `UNC.RDL.FREEZE`)

## Requirements

- **REQ-UNC.RDL.GEN.CMD-001** When `rdl.generate_dataset@1` runs with a family, version, seed and case count, the system shall return the generated records, and the same arguments shall return byte-identical records.
- **REQ-UNC.RDL.GEN.CMD-002** If an oracle handle is given, then the command shall return records finalised by `UNC.RDL.GEN.XCHECK` or its error, and otherwise records with cross-check status `pending`.
- **REQ-UNC.RDL.GEN.CMD-003** If the family or version is unknown, then the command shall fail with `unknown_family` listing the registered families.
- **REQ-UNC.RDL.GEN.CMD-004** If `cases` exceeds 10 000, then the command shall fail with `limit_exceeded`.

## Acceptance criteria

- **AC-1** Given `fid rdl generate-dataset --family ls.circle --family-version 1 --seed 1 --cases 10 --json` run twice, then the outputs are byte-identical and contain 10 records with status `pending`.
- **AC-2** Given the same with `--oracle root://in/oracle_ok.json`, then every record is `agreed`.
- **AC-3** (error path) Given `--family ls.torus`, then `unknown_family` with the list.
- **AC-4** Given AC-1, then no Record Journal entry is written and nothing is stored.

## Design

- **Registry command(s):** `rdl.generate_dataset@1` (R1; params as in Scope; returns `Vec<RdlDataset>` plus a generation report).
- **Data structures:** `FamilyRegistry` (BTreeMap of family ID to builder).
- **Algorithm:** dispatch; optional finalise.
- **Errors:** `unknown_family`, `limit_exceeded`, family and XCHECK errors.
- **Determinism:** D0.
- **Performance budget:** dispatch overhead negligible [unverified until measured].
- **Crate:** `fid-rdl` (command), calling `fid-rdl-gen`.

## Test fixtures

Reuses `fixtures/rdl/gen/xcheck/` (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl`, add registry command `rdl.generate_dataset@1` (R1; CLI `fid rdl generate-dataset`; MCP `rdl_generate_dataset`; `--json`). Params: family, family version, seed, cases (<= 10 000, else `limit_exceeded`), optional oracle import handle, `require_all`. Dispatch through a sorted family registry over `fid-rdl-gen` families; return records (cross-check `pending`) or records finalised by `xcheck::finalize` when an oracle handle is given; unknown family gives `unknown_family` with the list. Store nothing; no Record Journal entry (R1). Tests AC-1..AC-4. CONSTITUTION 8; agents get handles, never raw paths.

## Open questions

- None.

## Children

_Leaf: none._
